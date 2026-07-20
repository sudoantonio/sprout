use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchivePersistFault {
    None,
    #[cfg(test)]
    DiskFull,
    #[cfg(test)]
    AfterWrite,
    #[cfg(test)]
    BeforeRename,
    #[cfg(test)]
    AfterRename,
}

pub(crate) fn storage_key(archive_id: Uuid) -> String {
    format!("{}.archive", archive_id.simple())
}

pub(crate) fn validate_storage_key(value: &str) -> Result<(), AppError> {
    let Some(stem) = value.strip_suffix(".archive") else {
        return Err(AppError::Internal);
    };
    if stem.len() != 32
        || !stem
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AppError::Internal);
    }
    Ok(())
}

pub(crate) fn safe_path(root: &Path, key: &str) -> Result<PathBuf, AppError> {
    validate_storage_key(key)?;
    let path = root.join(key);
    if path.parent() != Some(root) {
        return Err(AppError::Internal);
    }
    Ok(path)
}

pub(crate) async fn persist(
    root: &Path,
    key: &str,
    bytes: &[u8],
    fault: ArchivePersistFault,
) -> std::io::Result<()> {
    let _ = fault;
    validate_storage_key(key).map_err(|_| std::io::Error::other("invalid archive key"))?;
    prepare_directory(root).await?;
    let final_path = root.join(key);
    if tokio::fs::symlink_metadata(&final_path).await.is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "archive destination already exists",
        ));
    }
    let temporary_path = root.join(format!(".tmp-{}", Uuid::new_v4().simple()));
    let mut renamed = false;
    let result = async {
        let mut temporary = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .await?;
        #[cfg(test)]
        if fault == ArchivePersistFault::DiskFull {
            return Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "injected disk full",
            ));
        }
        temporary.write_all(bytes).await?;
        temporary.sync_all().await?;
        ensure_regular_file(&temporary_path).await?;
        #[cfg(test)]
        if fault == ArchivePersistFault::AfterWrite {
            return Err(std::io::Error::other("injected crash after write"));
        }
        drop(temporary);
        #[cfg(test)]
        if fault == ArchivePersistFault::BeforeRename {
            return Err(std::io::Error::other("injected crash before rename"));
        }
        tokio::fs::rename(&temporary_path, &final_path).await?;
        renamed = true;
        ensure_regular_file(&final_path).await?;
        sync_directory(root).await?;
        #[cfg(test)]
        if fault == ArchivePersistFault::AfterRename {
            return Err(std::io::Error::other("injected crash after rename"));
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        if renamed {
            let _ = tokio::fs::remove_file(&final_path).await;
        }
        let _ = sync_directory(root).await;
    }
    result
}

pub(crate) async fn read(root: &Path, key: &str) -> Result<Vec<u8>, AppError> {
    let path = safe_path(root, key)?;
    ensure_regular_file(&path).await.map_err(io_error)?;
    tokio::fs::read(path).await.map_err(io_error)
}

pub(crate) async fn delete(root: &Path, key: &str) -> Result<(), AppError> {
    let path = safe_path(root, key)?;
    match tokio::fs::remove_file(path).await {
        Ok(()) => sync_directory(root).await.map_err(io_error),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

async fn prepare_directory(root: &Path) -> std::io::Result<()> {
    if let Some(parent) = root.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    match tokio::fs::symlink_metadata(root).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(std::io::Error::other(
            "archive root is not a real directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(root).await?;
            if let Some(parent) = root.parent() {
                sync_directory(parent).await?;
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn ensure_regular_file(path: &Path) -> std::io::Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other("archive path is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(std::io::Error::other("archive has multiple hard links"));
        }
    }
    Ok(())
}

async fn sync_directory(path: &Path) -> std::io::Result<()> {
    tokio::fs::File::open(path).await?.sync_all().await
}

fn io_error(error: std::io::Error) -> AppError {
    tracing::error!(kind = ?error.kind(), "retention archive filesystem operation failed");
    AppError::Internal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("sprout-retention-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn llr_08_6_crash_and_disk_full_leave_no_partial_archive() {
        for fault in [
            ArchivePersistFault::DiskFull,
            ArchivePersistFault::AfterWrite,
            ArchivePersistFault::BeforeRename,
            ArchivePersistFault::AfterRename,
        ] {
            let root = temp_root();
            let key = storage_key(Uuid::new_v4());
            assert!(
                persist(&root, &key, b"authenticated ciphertext", fault)
                    .await
                    .is_err()
            );
            if root.exists() {
                assert!(std::fs::read_dir(&root).unwrap().next().is_none());
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn llr_08_8_expired_archive_deletion_is_idempotent() {
        let root = temp_root();
        let key = storage_key(Uuid::new_v4());
        persist(&root, &key, b"ciphertext", ArchivePersistFault::None)
            .await
            .unwrap();
        delete(&root, &key).await.unwrap();
        delete(&root, &key).await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
