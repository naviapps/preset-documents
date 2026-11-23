//! Optional file adapter. The document codec has no dependency on this crate.
//! Hosts own paths, file dialogs, permissions and product import/export policy.
use preset_documents::{Error, MAXIMUM_BYTES};
use std::{
    fs::{File, Metadata},
    io::{Read, Write},
    path::Path,
};

#[cfg(target_os = "macos")]
mod finder_tags;
mod no_replace;
mod replacement;

pub use replacement::{
    DisplacedIdentity, ReplacementOutcome, ReplacementRevision, capture_replacement,
    replace_approved,
};

fn io(error: std::io::Error) -> Error {
    Error::FileAccess {
        kind: format!("{:?}", error.kind()),
    }
}
fn limit(maximum: usize) -> Result<(), Error> {
    if maximum == 0 || maximum > MAXIMUM_BYTES {
        Err(Error::InvalidConfiguration)
    } else {
        Ok(())
    }
}
fn read_bounded(mut reader: impl Read, maximum: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let mut block = [0; 8192];
    loop {
        // Read at most the remaining budget plus one sentinel byte. Do not trust
        // metadata: a file can grow after its initial size was observed.
        let count = block.len().min(maximum - bytes.len() + 1);
        let count = match reader.read(&mut block[..count]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            value => value.map_err(io)?,
        };
        if count == 0 {
            return Ok(bytes);
        }
        if count > maximum - bytes.len() {
            return Err(Error::InputTooLarge {
                byte_count: maximum + 1,
                maximum_byte_count: maximum,
            });
        }
        let required = bytes.len() + count;
        if required > bytes.capacity() {
            // Grow geometrically even when the source returns short reads.
            let capacity = required.max(bytes.capacity() * 2).min(maximum);
            bytes.reserve_exact(capacity - bytes.len());
        }
        bytes.extend_from_slice(&block[..count]);
    }
}
pub fn read(path: &Path, maximum: usize) -> Result<Vec<u8>, Error> {
    limit(maximum)?;
    let mut options = File::options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Inspect the opened object without waiting for a FIFO peer or acquiring a terminal.
        // A path-only metadata check would still permit a replacement before open.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let file = options.open(path).map_err(io)?;
    let metadata = file.metadata().map_err(io)?;
    if !metadata.is_file() {
        return Err(Error::FileAccess {
            kind: "NotRegularFile".into(),
        });
    }
    if metadata.len() > maximum as u64 {
        return Err(Error::InputTooLarge {
            byte_count: maximum + 1,
            maximum_byte_count: maximum,
        });
    }
    read_bounded(file, maximum)
}
fn destination_changed() -> Error {
    Error::FileAccess {
        kind: "DestinationChanged".into(),
    }
}

fn shares_destination_revision(initial: &Metadata, current: &Metadata) -> bool {
    if initial.len() != current.len() || initial.modified().ok() != current.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        initial.dev() == current.dev()
            && initial.ino() == current.ino()
            && initial.ctime() == current.ctime()
            && initial.ctime_nsec() == current.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        initial.created().ok() == current.created().ok()
    }
}

fn validate_destination(path: &Path) -> Result<Option<Metadata>, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata)),
        Ok(_) => Err(Error::FileAccess {
            kind: "NotRegularFile".into(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

/// Checks an output budget before borrowing or copying a write's bytes.
pub fn validate_write_size(byte_count: usize, maximum: usize) -> Result<(), Error> {
    limit(maximum)?;
    if byte_count > maximum {
        return Err(Error::OutputTooLarge {
            byte_count,
            maximum_byte_count: maximum,
        });
    }
    Ok(())
}

fn stage(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile, Error> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    temporary.write_all(bytes).map_err(io)?;
    Ok(temporary)
}

/// Atomic replacement of a regular file (or creation at an absent path).
/// Rejects observed final symbolic links instead of replacing the link itself.
/// Stages beside the destination and flushes content before the
/// replace; success means committed visibility, not guaranteed power-loss durability.
/// For an existing file, rechecks its revision separately before the rename.
/// An initially absent path uses exclusive creation and reports `AtomicNewUnsupported`
/// when the platform or volume lacks that operation.
/// macOS replacements preserve readable Finder tags on native xattr volumes.
/// Failed writes/replaces leave the destination intact and remove the temporary file.
pub fn write_atomically(path: &Path, bytes: &[u8], maximum: usize) -> Result<(), Error> {
    validate_write_size(bytes.len(), maximum)?;
    let initial = validate_destination(path)?;
    let temporary = stage(path, bytes)?;
    #[cfg(target_os = "macos")]
    if initial.is_some() {
        finder_tags::copy_to_staging(path, temporary.as_file())?;
    }
    publish_staged(temporary, path, initial.as_ref())
}

fn publish_staged(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    initial: Option<&Metadata>,
) -> Result<(), Error> {
    let Some(initial) = initial else {
        return match publish_new(temporary, path) {
            Err(Error::DestinationAlreadyExists) => Err(destination_changed()),
            result => result,
        };
    };

    temporary.as_file().sync_all().map_err(io)?;
    let current = validate_destination(path)?;
    if !current
        .as_ref()
        .is_some_and(|current| shares_destination_revision(initial, current))
    {
        return Err(destination_changed());
    }
    temporary.persist(path).map_err(|error| io(error.error))?;
    Ok(())
}

/// Publishes a complete new file without replacing any occupied destination.
/// A final symbolic link counts as occupied, even if it points to a missing file.
/// Unsupported exclusive publication never falls back to an overwrite or direct write.
pub fn write_new(path: &Path, bytes: &[u8], maximum: usize) -> Result<(), Error> {
    validate_write_size(bytes.len(), maximum)?;
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err(Error::DestinationAlreadyExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io(error)),
    }
    publish_new(stage(path, bytes)?, path)
}

fn publish_new(temporary: tempfile::NamedTempFile, path: &Path) -> Result<(), Error> {
    temporary.as_file().sync_all().map_err(io)?;
    no_replace::publish(temporary, path).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => Error::DestinationAlreadyExists,
        std::io::ErrorKind::Unsupported => Error::AtomicNewUnsupported,
        _ => io(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn new_file_does_not_replace_a_destination_created_during_staging() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        assert!(!path.exists());
        let temporary = stage(&path, b"staged preset").unwrap();
        let staged_path = temporary.path().to_owned();
        std::fs::write(&path, b"competing preset").unwrap();

        assert_eq!(
            publish_new(temporary, &path),
            Err(Error::DestinationAlreadyExists)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"competing preset");
        assert!(!staged_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn new_file_rejects_an_occupied_dangling_symbolic_link() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        let missing = root.path().join("missing");
        std::os::unix::fs::symlink(&missing, &path).unwrap();

        assert_eq!(
            write_new(&path, b"data", 4),
            Err(Error::DestinationAlreadyExists)
        );
        assert_eq!(std::fs::read_link(&path).unwrap(), missing);
        assert!(!missing.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn new_and_atomic_creation_preserve_a_dangling_junction() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        let missing = root.path().join("removed");
        let initial = validate_destination(&path).unwrap();
        let staged = stage(&path, b"staged").unwrap();
        std::fs::create_dir(&missing).unwrap();
        // Keep shell arguments literal; the owned temporary path is only the cwd.
        let result = std::process::Command::new("cmd.exe")
            .current_dir(root.path())
            .args(["/d", "/c", "mklink", "/J", "preset.preset", "removed"])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "cannot prepare owned junction fixture: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let target = std::fs::read_link(&path).unwrap();
        std::fs::remove_dir(&missing).unwrap();
        assert!(!path.exists());
        assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());

        assert_eq!(
            write_new(&path, b"data", 4),
            Err(Error::DestinationAlreadyExists)
        );
        assert!(publish_staged(staged, &path, initial.as_ref()).is_err());
        assert_eq!(std::fs::read_link(&path).unwrap(), target);
        assert!(!missing.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn changed_destination_is_not_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        let previous = root.path().join("previous.preset");
        std::fs::write(&path, b"original").unwrap();
        let initial = validate_destination(&path).unwrap();
        let mut staged = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        staged.write_all(b"staged").unwrap();
        let staged_path = staged.path().to_owned();
        std::fs::rename(&path, &previous).unwrap();
        std::fs::write(&path, b"replacement").unwrap();

        assert!(matches!(
            publish_staged(staged, &path, initial.as_ref()),
            Err(Error::FileAccess { kind }) if kind == "DestinationChanged"
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        assert_eq!(std::fs::read(&previous).unwrap(), b"original");
        assert!(!staged_path.exists());
    }

    #[test]
    fn in_place_edit_during_staging_is_not_overwritten() {
        use std::fs::FileTimes;
        use std::time::{Duration, SystemTime};

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        std::fs::write(&path, b"original").unwrap();
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(timestamp))
            .unwrap();
        let initial = validate_destination(&path).unwrap();
        let staged = stage(&path, b"staged").unwrap();
        let staged_path = staged.path().to_owned();
        let mut original = File::options().write(true).open(&path).unwrap();
        original.write_all(b"my edits").unwrap();
        original
            .set_times(FileTimes::new().set_modified(timestamp + Duration::from_secs(4)))
            .unwrap();
        drop(original);

        let edited = std::fs::metadata(&path).unwrap();
        assert_eq!(initial.as_ref().unwrap().len(), edited.len());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(initial.as_ref().unwrap().ino(), edited.ino());
        }
        assert!(matches!(
            publish_staged(staged, &path, initial.as_ref()),
            Err(Error::FileAccess { kind }) if kind == "DestinationChanged"
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"my edits");
        assert!(!staged_path.exists());
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn atomic_creation_preserves_a_competing_file_at_publication() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        let initial = validate_destination(&path).unwrap();
        let staged = stage(&path, b"staged").unwrap();
        let staged_path = staged.path().to_owned();
        std::fs::write(&path, b"competing preset").unwrap();

        assert!(matches!(
            publish_staged(staged, &path, initial.as_ref()),
            Err(Error::FileAccess { kind }) if kind == "DestinationChanged"
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"competing preset");
        assert!(!staged_path.exists());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn atomic_creation_preserves_a_competing_dangling_link_at_publication() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        let missing = root.path().join("missing");
        let initial = validate_destination(&path).unwrap();
        let staged = stage(&path, b"staged").unwrap();
        std::os::unix::fs::symlink(&missing, &path).unwrap();

        assert_eq!(
            publish_staged(staged, &path, initial.as_ref()),
            Err(destination_changed())
        );
        assert_eq!(std::fs::read_link(&path).unwrap(), missing);
        assert!(!missing.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replacement_does_not_require_reading_the_original_content() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preset.preset");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o200)).unwrap();

        write_atomically(&path, b"new", 4).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn named_pipe_reads_do_not_wait_for_a_writer() {
        const FIXTURE: &str = "PRESET_DOCUMENTS_NAMED_PIPE_FIXTURE";
        if let Some(path) = std::env::var_os(FIXTURE) {
            assert!(matches!(
                read(Path::new(&path), 4),
                Err(Error::FileAccess { kind }) if kind == "NotRegularFile"
            ));
            return;
        }

        let root = tempfile::tempdir().unwrap();
        let pipe = root.path().join("input.preset");
        let path = std::ffi::CString::new(pipe.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: the owned path is NUL-terminated and remains valid for the call.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let link = root.path().join("linked.preset");
        std::os::unix::fs::symlink(&pipe, &link).unwrap();
        for path in [&pipe, &link] {
            // A regression must fail within a bounded child, not strand the test runner.
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::named_pipe_reads_do_not_wait_for_a_writer",
                ])
                .env(FIXTURE, path)
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("A named-pipe read waited for a writer");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert!(status.success());
        }

        let file = root.path().join("regular.preset");
        std::fs::write(&file, b"data").unwrap();
        let link = root.path().join("regular-link.preset");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(read(&link, 4).unwrap(), b"data");
    }

    #[cfg(unix)]
    #[test]
    fn export_refuses_a_link_without_replacing_it_or_its_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("original.preset");
        std::fs::write(&target, b"old").unwrap();
        let link = root.path().join("export.preset");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            write_atomically(&link, b"new", 4),
            Err(Error::FileAccess { kind }) if kind == "NotRegularFile"
        ));
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn growing_stream_and_exact_limit_are_bounded() {
        assert_eq!(read_bounded(&b"1234"[..], 4).unwrap(), b"1234");
        assert!(matches!(
            read_bounded(&b"12345"[..], 4),
            Err(Error::InputTooLarge { byte_count: 5, .. })
        ));
    }
    #[test]
    fn replace_failure_and_budget_preserve_original_and_remove_temporary_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("日本語.preset");
        write_atomically(&path, b"old", 4).unwrap();
        assert!(write_atomically(&path, b"excess", 4).is_err());
        assert_eq!(read(&path, 4).unwrap(), b"old");
        write_atomically(&path, b"new", 4).unwrap();
        assert_eq!(read(&path, 4).unwrap(), b"new");
        let directory = root.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        assert!(write_atomically(&directory, b"new", 4).is_err());
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }
}
