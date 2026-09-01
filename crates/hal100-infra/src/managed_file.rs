use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ManagedFileError {
    #[error("managed file exceeds the {max_bytes} byte safety limit")]
    TooLarge { max_bytes: u64 },
    #[error("managed file path cannot be a symbolic link")]
    SymbolicLink,
    #[error("managed file operation failed: {0}")]
    Io(#[from] std::io::Error),
}

pub fn reject_symlink(path: &Path) -> Result<(), ManagedFileError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ManagedFileError::SymbolicLink),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ManagedFileError> {
    reject_symlink(path)?;
    let metadata = fs::metadata(path)?;
    if metadata.len() > max_bytes {
        return Err(ManagedFileError::TooLarge { max_bytes });
    }
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or_default());
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(ManagedFileError::TooLarge { max_bytes });
    }
    Ok(bytes)
}

pub fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> Result<(), ManagedFileError> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "managed file must have a parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    reject_symlink(path)?;
    let temporary = parent.join(format!(".hal100-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        write_new_file(&temporary, contents, mode)?;
        fs::rename(&temporary, path)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn write_new_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), ManagedFileError> {
    #[cfg(not(unix))]
    let _ = mode;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

pub fn existing_mode(path: &Path) -> Result<u32, ManagedFileError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::metadata(path)?.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(0o600)
    }
}

pub fn sync_directory(path: &Path) -> Result<(), ManagedFileError> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub fn backup_path(path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    path.with_file_name(format!("{file_name}.hal100-{timestamp}.bak"))
}

pub fn content_hash(contents: &[u8]) -> [u8; 32] {
    Sha256::digest(contents).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hal100-managed-file-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn atomic_write_uses_the_requested_private_mode() {
        let directory = TestDirectory::new();
        let path = directory.0.join("nested/credential.key");
        atomic_write(&path, b"secret", 0o600).expect("atomic write");

        assert_eq!(fs::read(&path).expect("read"), b"secret");
        #[cfg(unix)]
        assert_eq!(existing_mode(&path).expect("mode"), 0o600);
    }

    #[test]
    fn bounded_read_rejects_oversized_files() {
        let directory = TestDirectory::new();
        let path = directory.0.join("config.json");
        fs::write(&path, b"12345").expect("write fixture");

        assert!(matches!(
            read_bounded(&path, 4),
            Err(ManagedFileError::TooLarge { max_bytes: 4 })
        ));
    }
}
