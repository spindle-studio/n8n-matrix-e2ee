use std::path::{Path, PathBuf};

use matrix_sdk::authentication::matrix::MatrixSession;

pub(crate) fn matrix_store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("matrix_store")
}

pub(crate) fn matrix_session_path(data_dir: &Path) -> PathBuf {
    data_dir.join("matrix_session.json")
}

pub(crate) fn load_session(path: &Path) -> anyhow::Result<MatrixSession> {
    let serialized = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&serialized)?)
}

pub(crate) fn persist_session(path: &Path, session: Option<MatrixSession>) -> anyhow::Result<()> {
    let session =
        session.ok_or_else(|| anyhow::anyhow!("logged-in Matrix client has no session"))?;
    let serialized = serde_json::to_string(&session)?;
    let temporary_path = session_temporary_path(path);

    std::fs::write(&temporary_path, serialized)?;
    std::fs::rename(temporary_path, path)?;

    Ok(())
}

pub(crate) fn store_contains_data(path: &Path) -> anyhow::Result<bool> {
    Ok(std::fs::read_dir(path)?.next().transpose()?.is_some())
}

fn session_temporary_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn session_temporary_path_is_next_to_the_session_file() {
        let session_path = matrix_session_path(Path::new("/data"));
        assert_eq!(
            session_temporary_path(&session_path),
            Path::new("/data/matrix_session.json.tmp")
        );
        assert_eq!(
            matrix_store_path(Path::new("./data")),
            Path::new("./data/matrix_store")
        );
    }

    #[test]
    fn empty_store_directory_has_no_data() {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let temporary_directory = std::env::temp_dir().join(format!(
            "matrix-store-test-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temporary_directory)
            .expect("temporary directory should be created");

        assert!(
            !store_contains_data(&temporary_directory)
                .expect("temporary directory should be readable")
        );

        std::fs::remove_dir(&temporary_directory).expect("temporary directory should be removed");
    }
}
