use std::{
    fmt::Write as _,
    path::Path,
    sync::{Arc, Mutex},
};

use matrix_sdk::ruma::{RoomId, events::room::MediaSource};
use mime::Mime;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use tracing::warn;

const CREATE_MEDIA_CACHE_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS media_cache (
    room_id       TEXT NOT NULL,
    is_encrypted  INTEGER NOT NULL,
    content_type  TEXT NOT NULL,
    size_bytes    INTEGER NOT NULL,
    sha256        TEXT NOT NULL,
    media_source  TEXT NOT NULL,
    created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (room_id, is_encrypted, content_type, size_bytes, sha256)
);
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaCacheKey {
    pub(crate) room_id: String,
    pub(crate) is_encrypted: bool,
    pub(crate) content_type: String,
    pub(crate) size_bytes: u64,
    pub(crate) sha256: String,
}

impl MediaCacheKey {
    pub(crate) fn new(
        room_id: &RoomId,
        is_encrypted: bool,
        content_type: &Mime,
        data: &[u8],
    ) -> Self {
        Self {
            room_id: room_id.as_str().to_owned(),
            is_encrypted,
            content_type: content_type.essence_str().to_owned(),
            size_bytes: data.len() as u64,
            sha256: sha256_hex(data),
        }
    }
}

#[derive(Clone)]
pub(crate) struct MediaCache {
    connection: Arc<Mutex<Connection>>,
}

impl MediaCache {
    pub(crate) fn open(path: &Path) -> anyhow::Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> anyhow::Result<Self> {
        connection.execute_batch(CREATE_MEDIA_CACHE_TABLE_SQL)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub(crate) fn get(&self, key: &MediaCacheKey) -> Option<MediaSource> {
        let connection = match self.connection.lock() {
            Ok(connection) => connection,
            Err(error) => {
                warn!(%error, room_id = %key.room_id, "media cache mutex is poisoned during lookup");
                return None;
            }
        };

        let size_bytes = match i64::try_from(key.size_bytes) {
            Ok(size_bytes) => size_bytes,
            Err(_) => return None,
        };

        let serialized: Option<String> = match connection
            .query_row(
                "SELECT media_source FROM media_cache
                 WHERE room_id = ?1
                   AND is_encrypted = ?2
                   AND content_type = ?3
                   AND size_bytes = ?4
                   AND sha256 = ?5",
                params![
                    &key.room_id,
                    i64::from(key.is_encrypted),
                    &key.content_type,
                    size_bytes,
                    &key.sha256,
                ],
                |row| row.get(0),
            )
            .optional()
        {
            Ok(serialized) => serialized,
            Err(error) => {
                warn!(%error, room_id = %key.room_id, sha256 = %key.sha256, "failed to read media_cache entry");
                return None;
            }
        };

        let serialized = serialized?;
        match serde_json::from_str(&serialized) {
            Ok(source) => Some(source),
            Err(error) => {
                warn!(%error, room_id = %key.room_id, sha256 = %key.sha256, "failed to deserialize cached MediaSource");
                None
            }
        }
    }

    pub(crate) fn put(&self, key: &MediaCacheKey, source: &MediaSource) {
        let serialized = match serde_json::to_string(source) {
            Ok(serialized) => serialized,
            Err(error) => {
                warn!(%error, room_id = %key.room_id, sha256 = %key.sha256, "failed to serialize MediaSource for media_cache");
                return;
            }
        };

        let size_bytes = match i64::try_from(key.size_bytes) {
            Ok(size_bytes) => size_bytes,
            Err(_) => return,
        };

        let connection = match self.connection.lock() {
            Ok(connection) => connection,
            Err(error) => {
                warn!(%error, room_id = %key.room_id, "media cache mutex is poisoned during insert");
                return;
            }
        };

        if let Err(error) = connection.execute(
            "INSERT OR REPLACE INTO media_cache (
                 room_id,
                 is_encrypted,
                 content_type,
                 size_bytes,
                 sha256,
                 media_source
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &key.room_id,
                i64::from(key.is_encrypted),
                &key.content_type,
                size_bytes,
                &key.sha256,
                serialized,
            ],
        ) {
            warn!(%error, room_id = %key.room_id, sha256 = %key.sha256, "failed to write media_cache entry");
        }
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use matrix_sdk::ruma::{owned_mxc_uri, room_id};

    use super::*;

    #[test]
    fn key_normalizes_mime_and_computes_sha256_and_size() {
        let room_id = room_id!("!series:example.org");
        let content_type: Mime = "IMAGE/PNG; charset=utf-8".parse().unwrap();
        let data = b"poster-bytes";

        let key = MediaCacheKey::new(room_id, true, &content_type, data);

        assert_eq!(key.room_id, "!series:example.org");
        assert!(key.is_encrypted);
        assert_eq!(key.content_type, "image/png");
        assert_eq!(key.size_bytes, 12);
        assert_eq!(
            key.sha256,
            "db4cd92bd09403b5e8535467a5099c222bbef177d68eaf8d21dc1db0a1ec1978"
        );
    }

    #[test]
    fn media_cache_persists_and_round_trips_plain_and_encrypted_sources() {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "media-cache-test-{}-{unique_suffix}.sqlite3",
            std::process::id()
        ));

        let room_a = room_id!("!room-a:example.org");
        let room_b = room_id!("!room-b:example.org");
        let png_mime: Mime = "image/png".parse().unwrap();
        let jpeg_mime: Mime = "image/jpeg".parse().unwrap();
        let bytes = b"sonarr-poster";

        let plain_key = MediaCacheKey::new(room_a, false, &png_mime, bytes);
        let plain_source = MediaSource::Plain(owned_mxc_uri!("mxc://example.org/plain-poster"));

        let encrypted_key = MediaCacheKey::new(room_a, true, &png_mime, bytes);
        let encrypted_source: MediaSource = serde_json::from_value(serde_json::json!({
            "file": {
                "url": "mxc://example.org/encrypted-poster",
                "key": {
                    "kty": "oct",
                    "key_ops": ["encrypt", "decrypt"],
                    "alg": "A256CTR",
                    "k": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    "ext": true
                },
                "iv": "AAAAAAAAAAAAAAAAAAAAAA",
                "hashes": {
                    "sha256": "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU"
                },
                "v": "v2"
            }
        }))
        .expect("encrypted MediaSource should deserialize");

        {
            let cache = MediaCache::open(&db_path).expect("cache should open");
            assert!(cache.get(&plain_key).is_none());

            cache.put(&plain_key, &plain_source);
            cache.put(&encrypted_key, &encrypted_source);
        }

        // Reopen from disk to verify persistence across restarts
        let reopened = MediaCache::open(&db_path).expect("cache should reopen");
        assert_eq!(
            serde_json::to_value(reopened.get(&plain_key).unwrap()).unwrap(),
            serde_json::to_value(&plain_source).unwrap()
        );
        assert_eq!(
            serde_json::to_value(reopened.get(&encrypted_key).unwrap()).unwrap(),
            serde_json::to_value(&encrypted_source).unwrap()
        );

        // Different room, MIME type, or content does not hit the cache
        let other_room_key = MediaCacheKey::new(room_b, true, &png_mime, bytes);
        assert!(reopened.get(&other_room_key).is_none());

        let other_mime_key = MediaCacheKey::new(room_a, true, &jpeg_mime, bytes);
        assert!(reopened.get(&other_mime_key).is_none());

        let other_bytes_key = MediaCacheKey::new(room_a, true, &png_mime, b"different-poster");
        assert!(reopened.get(&other_bytes_key).is_none());

        drop(reopened);
        let _ = std::fs::remove_file(&db_path);
    }
}
