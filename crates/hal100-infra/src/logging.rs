use std::{
    fmt, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

const LOG_FILE_MAX_BYTES: u64 = 5 * 1024 * 1024;
const LOG_ARCHIVE_COUNT: usize = 6;
const ACTIVE_LOG_FILENAME: &str = "hal100.jsonl";
const DEFAULT_LOG_FILTER: &str =
    "warn,hal100_desktop_lib=info,hal100_core=info,hal100_infra=info,hal100_platform=info";

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("failed to create the log directory: {0}")]
    CreateDirectory(#[source] std::io::Error),
    #[error("failed to initialize the size-limited log writer: {0}")]
    InitializeAppender(#[source] std::io::Error),
    #[error("failed to install the global log subscriber: {0}")]
    InstallSubscriber(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Keeps the asynchronous log writer alive until the desktop runtime shuts down.
pub struct LoggingGuard {
    _worker: WorkerGuard,
}

/// A formatting wrapper for values that must never be written to diagnostics.
pub struct Redacted<T>(pub T);

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

pub fn init_structured_logging(log_dir: impl AsRef<Path>) -> Result<LoggingGuard, LoggingError> {
    let log_dir = log_dir.as_ref();
    fs::create_dir_all(log_dir).map_err(LoggingError::CreateDirectory)?;
    secure_directory_permissions(log_dir).map_err(LoggingError::CreateDirectory)?;

    let appender = SizeRollingFileAppender::new(log_dir, LOG_FILE_MAX_BYTES, LOG_ARCHIVE_COUNT)
        .map_err(LoggingError::InitializeAppender)?;
    let (writer, worker) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::builder()
        .with_env_var("HAL100_LOG")
        .try_from_env()
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .json()
        .flatten_event(true)
        .with_ansi(false)
        .try_init()
        .map_err(LoggingError::InstallSubscriber)?;

    Ok(LoggingGuard { _worker: worker })
}

struct SizeRollingFileAppender {
    directory: PathBuf,
    active_file: Option<File>,
    current_size: u64,
    max_bytes: u64,
    archive_count: usize,
}

impl SizeRollingFileAppender {
    fn new(directory: &Path, max_bytes: u64, archive_count: usize) -> io::Result<Self> {
        if max_bytes == 0 || archive_count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log size and archive count must be non-zero",
            ));
        }

        let active_path = directory.join(ACTIVE_LOG_FILENAME);
        let active_file = open_secure_append_file(&active_path)?;
        let current_size = active_file.metadata()?.len();
        Ok(Self {
            directory: directory.to_owned(),
            active_file: Some(active_file),
            current_size,
            max_bytes,
            archive_count,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut active_file) = self.active_file.take()
            && let Err(error) = active_file.flush()
        {
            self.active_file = Some(active_file);
            return Err(error);
        }

        let rotation_result = (|| {
            remove_if_exists(&self.archive_path(self.archive_count))?;
            for index in (1..self.archive_count).rev() {
                rename_if_exists(&self.archive_path(index), &self.archive_path(index + 1))?;
            }
            rename_if_exists(&self.active_path(), &self.archive_path(1))
        })();

        let active_file = open_secure_append_file(&self.active_path())?;
        self.current_size = active_file.metadata()?.len();
        self.active_file = Some(active_file);
        rotation_result
    }

    fn active_path(&self) -> PathBuf {
        self.directory.join(ACTIVE_LOG_FILENAME)
    }

    fn archive_path(&self, index: usize) -> PathBuf {
        self.directory.join(format!("hal100.{index}.jsonl"))
    }
}

impl Write for SizeRollingFileAppender {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.current_size > 0
            && self.current_size.saturating_add(buffer.len() as u64) > self.max_bytes
        {
            self.rotate()?;
        }

        let written = self.active_file_mut()?.write(buffer)?;
        self.current_size = self.current_size.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.active_file_mut()?.flush()
    }
}

impl SizeRollingFileAppender {
    fn active_file_mut(&mut self) -> io::Result<&mut File> {
        self.active_file.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "active log file is unavailable after a rotation failure",
            )
        })
    }
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn rename_if_exists(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn open_secure_append_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    secure_file_permissions(path)?;
    Ok(file)
}

#[cfg(unix)]
fn secure_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn secure_directory_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn secure_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn secure_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::SystemTime,
    };

    use super::*;

    static NEXT_TEST_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn redacted_values_never_render_the_inner_secret() {
        let value = Redacted("sk-not-for-logs");

        assert_eq!(format!("{value}"), "[REDACTED]");
        assert_eq!(format!("{value:?}"), "[REDACTED]");
    }

    #[test]
    fn rotates_before_crossing_the_size_limit_and_caps_archives() {
        let directory = TestDirectory::new();
        let mut appender =
            SizeRollingFileAppender::new(directory.path(), 8, 2).expect("create test appender");

        for line in [
            b"one\n".as_slice(),
            b"two\n".as_slice(),
            b"three\n".as_slice(),
            b"four\n".as_slice(),
        ] {
            appender.write_all(line).expect("write test line");
        }
        appender.flush().expect("flush test appender");

        assert_eq!(
            fs::read_to_string(directory.path().join("hal100.jsonl")).expect("active log"),
            "four\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("hal100.1.jsonl")).expect("new archive"),
            "three\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("hal100.2.jsonl")).expect("old archive"),
            "one\ntwo\n"
        );
        assert!(!directory.path().join("hal100.3.jsonl").exists());
    }

    #[cfg(unix)]
    #[test]
    fn restricts_log_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let _appender =
            SizeRollingFileAppender::new(directory.path(), 128, 1).expect("test appender");
        let mode = fs::metadata(directory.path().join(ACTIVE_LOG_FILENAME))
            .expect("log metadata")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, 0o600);
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hal100-logging-test-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("current time")
                    .as_nanos(),
                NEXT_TEST_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&path).expect("create logging test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
