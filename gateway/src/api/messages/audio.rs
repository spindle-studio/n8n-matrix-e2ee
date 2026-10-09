use std::{io::Cursor, time::Duration};

use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, header},
};
use matrix_sdk::{
    Room,
    ruma::{
        UInt,
        events::room::{
            MediaSource,
            message::{AudioInfo, AudioMessageEventContent, MessageType, RoomMessageEventContent},
            message::{
                UnstableAmplitude, UnstableAudioDetailsContentBlock, UnstableVoiceContentBlock,
            },
        },
    },
};
use ogg::PacketReader;
use opus_decoder::OpusDecoder;
use serde::Deserialize;
use tracing::{error, info};

use crate::{
    api::{
        auth::authorize_request,
        error::{ApiResult, client_error, server_error},
        room::{get_known_room, parse_room_id},
    },
    gateway::AppState,
};

use super::{MessageActionResponse, event::GatewayMessageType, message_action_response};

const OPUS_SAMPLE_RATE: u32 = 48_000;
const WAVEFORM_BUCKET_COUNT: usize = 60;
const WAVEFORM_COMPRESSION_EXPONENT: f64 = 0.25;
#[derive(Debug, Deserialize)]
pub(super) struct SendAudioQuery {
    room_id: String,
    file_name: Option<String>,
    caption: Option<String>,
}

fn downmix_pcm(pcm: &[i16], channels: usize) -> Vec<i16> {
    pcm.chunks_exact(channels)
        .map(|frame| {
            (frame.iter().map(|sample| i32::from(*sample)).sum::<i32>() / channels as i32) as i16
        })
        .collect()
}

fn normalize_waveform(mono_samples: &[i16]) -> Vec<UnstableAmplitude> {
    let bucket_count = WAVEFORM_BUCKET_COUNT.min(mono_samples.len());
    if bucket_count == 0 {
        return Vec::new();
    }
    let bucket_length = mono_samples.len() / bucket_count;
    let peaks = mono_samples
        .chunks_exact(bucket_length)
        .take(bucket_count)
        .map(|bucket| {
            bucket
                .iter()
                .map(|sample| sample.unsigned_abs())
                .max()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let max_peak = peaks.iter().copied().max().unwrap_or_default();

    peaks
        .into_iter()
        .map(|peak| {
            let amplitude = if max_peak == 0 {
                0
            } else {
                ((f64::from(peak) / f64::from(max_peak)).powf(WAVEFORM_COMPRESSION_EXPONENT)
                    * f64::from(UnstableAmplitude::MAX))
                .round() as u16
            };
            UnstableAmplitude::new(amplitude)
        })
        .collect()
}

struct AudioMetadata {
    duration: Duration,
    waveform: Vec<UnstableAmplitude>,
}

pub(super) async fn send_audio(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SendAudioQuery>,
    body: Bytes,
) -> ApiResult<Json<MessageActionResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    validate_audio_content_type(&headers)?;
    if body.is_empty() {
        return Err(client_error("audio body must not be empty"));
    }

    let metadata = extract_audio_metadata(&body).map_err(|message| client_error(message))?;
    let room_id = parse_room_id(&query.room_id)?;
    let room = get_known_room(&state.client, &room_id)?;
    let file_name = audio_file_name(query.file_name.as_deref());
    let message_body = audio_body(query.caption.as_deref());
    let source = upload_audio_media(&state.client, &room, &headers, body.to_vec()).await?;
    let content = audio_message_content(
        message_body,
        file_name.clone(),
        source,
        body.len() as u64,
        metadata.duration,
        metadata.waveform,
    );

    info!(room_id = %room_id, file_name, message_type = "audio", "sending Matrix audio message");
    let response = room
        .send(RoomMessageEventContent::new(MessageType::Audio(content)))
        .await
        .map_err(|error| {
            error!(%error, room_id = %room_id, "failed to send Matrix audio message");
            server_error("failed to send Matrix audio message")
        })?;

    Ok(message_action_response(
        room_id.to_string(),
        response.response.event_id.to_string(),
        GatewayMessageType::Audio,
    ))
}

fn validate_audio_content_type(headers: &HeaderMap) -> ApiResult<()> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| client_error("Content-Type header is required"))?
        .parse::<mime::Mime>()
        .map_err(|_| client_error("Content-Type header must be a valid MIME type"))?;
    if content_type.essence_str() != "audio/ogg" {
        return Err(client_error(
            "Content-Type must be audio/ogg for Ogg/Opus audio",
        ));
    }
    Ok(())
}

fn extract_audio_metadata(data: &[u8]) -> Result<AudioMetadata, &'static str> {
    let mut packets = PacketReader::new(Cursor::new(data));
    let header = packets
        .read_packet()
        .map_err(|_| "audio body must be a valid Ogg container")?
        .ok_or("audio body must be a valid Ogg container")?;
    let (channels, pre_skip) = parse_opus_header(&header.data)?;
    let tags = packets
        .read_packet()
        .map_err(|_| "audio body must be a valid Ogg/Opus stream")?
        .ok_or("audio body must be a valid Ogg/Opus stream")?;
    if !tags.data.starts_with(b"OpusTags") {
        return Err("audio body must be an Ogg/Opus stream");
    }

    let mut decoder = OpusDecoder::new(OPUS_SAMPLE_RATE, channels)
        .map_err(|_| "audio body contains an unsupported Opus stream")?;
    let mut mono_samples = Vec::new();
    let mut total_samples = 0_u64;
    let mut final_granule_position = None;
    while let Some(packet) = packets
        .read_packet()
        .map_err(|_| "audio body must be a valid Ogg/Opus stream")?
    {
        if packet.data.is_empty() {
            continue;
        }
        let mut pcm = vec![0_i16; decoder.max_frame_size_per_channel() * channels];
        let samples = decoder
            .decode(&packet.data, &mut pcm, false)
            .map_err(|_| "audio body contains an undecodable Opus packet")?;
        let sample_count = samples as u64;
        mono_samples.extend(downmix_pcm(&pcm[..samples * channels], channels));
        total_samples = total_samples.saturating_add(sample_count);
        if packet.absgp_page() != u64::MAX {
            final_granule_position = Some(packet.absgp_page());
        }
    }
    let total_samples = final_granule_position
        .unwrap_or(total_samples)
        .saturating_sub(u64::from(pre_skip));
    let pre_skip = usize::from(pre_skip).min(mono_samples.len());
    mono_samples.drain(..pre_skip);
    mono_samples.truncate(total_samples.min(usize::MAX as u64) as usize);
    if total_samples == 0 || mono_samples.is_empty() {
        return Err("audio body must contain Opus audio");
    }

    Ok(AudioMetadata {
        duration: Duration::from_secs_f64(total_samples as f64 / f64::from(OPUS_SAMPLE_RATE)),
        waveform: normalize_waveform(&mono_samples),
    })
}

fn parse_opus_header(header: &[u8]) -> Result<(usize, u16), &'static str> {
    if header.len() < 19 || &header[..8] != b"OpusHead" {
        return Err("audio body must be an Ogg/Opus stream");
    }
    let channels = usize::from(header[9]);
    if !(1..=2).contains(&channels) {
        return Err("audio body must contain mono or stereo Opus audio");
    }
    Ok((channels, u16::from_le_bytes([header[10], header[11]])))
}

fn audio_file_name(file_name: Option<&str>) -> String {
    let name = file_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("audio-message.ogg");
    if name.to_ascii_lowercase().ends_with(".ogg") {
        name.to_owned()
    } else {
        format!("{name}.ogg")
    }
}

fn audio_body(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}

async fn upload_audio_media(
    client: &matrix_sdk::Client,
    room: &Room,
    headers: &HeaderMap,
    data: Vec<u8>,
) -> ApiResult<MediaSource> {
    if room.latest_encryption_state().await.map_err(|error| {
        error!(%error, room_id = %room.room_id(), "failed to inspect Matrix room encryption state");
        server_error("failed to inspect Matrix room encryption state")
    })?.is_encrypted() {
        let mut reader = Cursor::new(data);
        let encrypted = client.upload_encrypted_file(&mut reader).await.map_err(|error| {
            error!(%error, room_id = %room.room_id(), "failed to upload encrypted Matrix audio media");
            server_error("failed to upload Matrix audio media")
        })?;
        Ok(MediaSource::Encrypted(Box::new(encrypted)))
    } else {
        let content_type: mime::Mime = headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap().parse().unwrap();
        let upload = client.media().upload(&content_type, data, None).await.map_err(|error| {
            error!(%error, room_id = %room.room_id(), "failed to upload Matrix audio media");
            server_error("failed to upload Matrix audio media")
        })?;
        Ok(MediaSource::Plain(upload.content_uri))
    }
}

fn audio_message_content(
    body: String,
    file_name: String,
    source: MediaSource,
    size_bytes: u64,
    duration: Duration,
    waveform: Vec<UnstableAmplitude>,
) -> AudioMessageEventContent {
    let mut info = AudioInfo::new();
    info.duration = Some(duration);
    info.mimetype = Some("audio/ogg".to_owned());
    info.size = UInt::new(size_bytes);

    let mut content = AudioMessageEventContent::new(body, source);
    content.filename = Some(file_name);
    content.info = Some(Box::new(info));
    content.audio = Some(UnstableAudioDetailsContentBlock::new(duration, waveform));
    content.voice = Some(UnstableVoiceContentBlock::new());
    content
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::owned_mxc_uri;

    use super::*;

    #[test]
    fn opus_header_requires_mono_or_stereo_audio() {
        let mut header = b"OpusHead\x01\x01\x00\x00\x80\xbb\x00\x00\x00\x00\x00".to_vec();
        assert_eq!(parse_opus_header(&header).unwrap(), (1, 0));
        header[9] = 3;
        assert!(parse_opus_header(&header).is_err());
    }

    #[test]
    fn voice_compatible_audio_message_contains_standard_metadata() {
        let content = audio_message_content(
            "Audio message".to_owned(),
            "audio.ogg".to_owned(),
            MediaSource::Plain(owned_mxc_uri!("mxc://example.org/audio")),
            42,
            Duration::from_secs(3),
            vec![UnstableAmplitude::new(0), UnstableAmplitude::new(1024)],
        );

        assert_eq!(content.filename.as_deref(), Some("audio.ogg"));
        assert_eq!(
            content.info.as_ref().unwrap().mimetype.as_deref(),
            Some("audio/ogg")
        );
        assert_eq!(content.info.as_ref().unwrap().size, UInt::new(42));
        assert_eq!(
            serde_json::to_value(RoomMessageEventContent::new(MessageType::Audio(content)))
                .unwrap(),
            serde_json::json!({
                "body": "Audio message",
                "filename": "audio.ogg",
                "url": "mxc://example.org/audio",
                "info": {"duration": 3000, "mimetype": "audio/ogg", "size": 42},
                "org.matrix.msc1767.audio": {"duration": 3000, "waveform": [0, 1024]},
                "org.matrix.msc3245.voice": {},
                "msgtype": "m.audio"
            })
        );
    }

    #[test]
    fn audio_body_is_empty_without_a_caption() {
        assert_eq!(audio_body(None), "");
        assert_eq!(audio_body(Some("  Reply  ")), "Reply");
    }

    #[test]
    fn waveform_uses_fixed_buckets_and_fourth_root_compression() {
        let mut samples = vec![0_i16; WAVEFORM_BUCKET_COUNT * 2];
        samples[0] = 1_000;
        samples[2] = 4_000;
        let waveform = normalize_waveform(&samples);

        assert_eq!(waveform.len(), WAVEFORM_BUCKET_COUNT);
        assert_eq!(u64::from(waveform[0].get()), 724);
        assert_eq!(u64::from(waveform[1].get()), 1024);
        assert_eq!(u64::from(waveform[2].get()), 0);
    }

    #[test]
    fn downmixing_averages_channels() {
        assert_eq!(downmix_pcm(&[1_000, 3_000, -2_000, 2_000], 2), [2_000, 0]);
    }
}
