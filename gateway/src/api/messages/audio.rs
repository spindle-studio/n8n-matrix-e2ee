use std::{io::Cursor, time::Duration};

use axum::{
    Json,
    extract::{Multipart, State},
    http::HeaderMap,
};
use matrix_sdk::ruma::{
    UInt,
    events::{
        Mentions,
        room::{
            MediaSource,
            message::{
                AudioInfo, AudioMessageEventContent, MessageType, RoomMessageEventContent,
                UnstableAmplitude, UnstableAudioDetailsContentBlock, UnstableVoiceContentBlock,
            },
        },
    },
};
use mime::Mime;
use ogg::PacketReader;
use opus_decoder::OpusDecoder;
use tracing::{error, info};

use crate::{
    api::{
        auth::authorize_request,
        error::{ApiResult, client_error, server_error},
        room::resolve_known_room,
    },
    gateway::AppState,
};

use super::{
    MessageActionResponse, RelatesTo, event::GatewayMessageType, extract_multipart_media,
    inject_relates_to_value, into_raw_room_message_content, message_action_response,
    resolve_media_relates_to, resolve_mentions, upload_room_media,
};

const OPUS_SAMPLE_RATE: u32 = 48_000;
const WAVEFORM_BUCKET_COUNT: usize = 60;
const WAVEFORM_COMPRESSION_EXPONENT: f64 = 0.25;

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
    multipart: Multipart,
) -> ApiResult<Json<MessageActionResponse>> {
    let account = authorize_request(&state, &headers)?;
    let upload = extract_multipart_media(multipart).await?;
    let content_type = validate_audio_content_type(upload.content_type)?;
    if upload.data.is_empty() {
        return Err(client_error("audio body must not be empty"));
    }

    let mentions = resolve_mentions(upload.metadata.mentions)?;
    let relates_to = resolve_media_relates_to(
        upload.metadata.relation_type,
        upload.metadata.target_event_id.as_deref(),
    )?;
    let metadata = extract_audio_metadata(&upload.data).map_err(|message| client_error(message))?;
    let room = resolve_known_room(&account.client, &upload.metadata.room_id).await?;
    let room_id = room.room_id().to_owned();
    let file_name = audio_file_name(
        upload
            .metadata
            .file_name
            .as_deref()
            .or(upload.part_file_name.as_deref()),
    );
    let message_body = audio_body(upload.metadata.body.as_deref());
    let source = upload_room_media(
        &account.client,
        &room,
        &content_type,
        upload.data.to_vec(),
        "failed to upload Matrix audio media",
    )
    .await?;
    let payload = audio_room_message_payload(
        message_body,
        file_name.clone(),
        source,
        upload.data.len() as u64,
        metadata.duration,
        metadata.waveform,
        mentions,
        relates_to.as_ref(),
    )?;
    let raw_content = into_raw_room_message_content(&payload)?;

    info!(account = %account.account_id, room_id = %room_id, file_name, message_type = "audio", "sending Matrix audio message");
    let response = room
        .send_raw("m.room.message", raw_content)
        .await
        .map_err(|error| {
            error!(account = %account.account_id, %error, room_id = %room_id, "failed to send Matrix audio message");
            server_error("failed to send Matrix audio message")
        })?;

    Ok(message_action_response(
        room_id.to_string(),
        response.response.event_id.to_string(),
        GatewayMessageType::Audio,
    ))
}

fn validate_audio_content_type(content_type: Option<Mime>) -> ApiResult<Mime> {
    let content_type =
        content_type.ok_or_else(|| client_error("media part Content-Type header is required"))?;
    if content_type.essence_str() != "audio/ogg" {
        return Err(client_error(
            "media part Content-Type must be audio/ogg for Ogg/Opus audio",
        ));
    }
    Ok(content_type)
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

fn audio_room_message_content(
    body: String,
    file_name: String,
    source: MediaSource,
    size_bytes: u64,
    duration: Duration,
    waveform: Vec<UnstableAmplitude>,
    mentions: Mentions,
) -> RoomMessageEventContent {
    let mut info = AudioInfo::new();
    info.duration = Some(duration);
    info.mimetype = Some("audio/ogg".to_owned());
    info.size = UInt::new(size_bytes);

    let mut content = AudioMessageEventContent::new(body, source);
    content.filename = Some(file_name);
    content.info = Some(Box::new(info));
    content.audio = Some(UnstableAudioDetailsContentBlock::new(duration, waveform));
    content.voice = Some(UnstableVoiceContentBlock::new());

    let mut message = RoomMessageEventContent::new(MessageType::Audio(content));
    message.mentions = Some(mentions);
    message
}

fn audio_room_message_payload(
    body: String,
    file_name: String,
    source: MediaSource,
    size_bytes: u64,
    duration: Duration,
    waveform: Vec<UnstableAmplitude>,
    mentions: Mentions,
    relates_to: Option<&RelatesTo>,
) -> ApiResult<serde_json::Value> {
    let content = audio_room_message_content(
        body,
        file_name,
        source,
        size_bytes,
        duration,
        waveform,
        mentions,
    );
    let json = serde_json::to_value(content)
        .map_err(|_| server_error("failed to serialize Matrix audio message content"))?;
    Ok(inject_relates_to_value(json, relates_to))
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::owned_mxc_uri;

    use super::super::MentionsRequest;
    use super::*;

    #[test]
    fn opus_header_requires_mono_or_stereo_audio() {
        let mut header = b"OpusHead\x01\x01\x00\x00\x80\xbb\x00\x00\x00\x00\x00".to_vec();
        assert_eq!(parse_opus_header(&header).unwrap(), (1, 0));
        header[9] = 3;
        assert!(parse_opus_header(&header).is_err());
    }

    #[test]
    fn voice_compatible_audio_message_contains_standard_metadata_and_mentions() {
        let mentions = resolve_mentions(Some(MentionsRequest {
            user_ids: Some(vec!["@alice:example.org".to_owned()]),
            room: Some(true),
        }))
        .unwrap();
        let content = audio_room_message_content(
            "Audio message".to_owned(),
            "audio.ogg".to_owned(),
            MediaSource::Plain(owned_mxc_uri!("mxc://example.org/audio")),
            42,
            Duration::from_secs(3),
            vec![UnstableAmplitude::new(0), UnstableAmplitude::new(1024)],
            mentions,
        );

        assert_eq!(
            serde_json::to_value(content).unwrap(),
            serde_json::json!({
                "body": "Audio message",
                "filename": "audio.ogg",
                "url": "mxc://example.org/audio",
                "info": {"duration": 3000, "mimetype": "audio/ogg", "size": 42},
                "org.matrix.msc1767.audio": {"duration": 3000, "waveform": [0, 1024]},
                "org.matrix.msc3245.voice": {},
                "msgtype": "m.audio",
                "m.mentions": {
                    "user_ids": ["@alice:example.org"],
                    "room": true
                }
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

    #[test]
    fn audio_payload_injects_relates_to() {
        let reply = RelatesTo::Reply {
            event_id: "$target:example.org".to_owned(),
        };
        let payload = audio_room_message_payload(
            "Voice reply".to_owned(),
            "audio.ogg".to_owned(),
            MediaSource::Plain(owned_mxc_uri!("mxc://example.org/audio")),
            42,
            Duration::from_secs(3),
            vec![UnstableAmplitude::new(0), UnstableAmplitude::new(1024)],
            Mentions::new(),
            Some(&reply),
        )
        .unwrap();

        assert_eq!(
            payload["m.relates_to"],
            serde_json::json!({
                "m.in_reply_to": {
                    "event_id": "$target:example.org"
                }
            })
        );
    }
}
