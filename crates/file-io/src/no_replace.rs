//! Commit a synced staging file with one native no-replace operation.
use std::{io, path::Path};
use tempfile::NamedTempFile;

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn publish(mut staged: NamedTempFile, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let from = CString::new(staged.path().as_os_str().as_bytes())?;
    let to = CString::new(destination.as_os_str().as_bytes())?;
    // SAFETY: Both paths are NUL-terminated and valid for the duration of the call.
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    // SAFETY: Both paths are NUL-terminated. Use the syscall so this does not
    // require the newer glibc renameat2 wrapper.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        let code = error.raw_os_error();
        #[cfg(target_os = "macos")]
        let unsupported = matches!(code, Some(libc::ENOTSUP) | Some(libc::EOPNOTSUPP));
        #[cfg(target_os = "linux")]
        let unsupported = matches!(code, Some(libc::ENOSYS) | Some(libc::EINVAL));
        return if unsupported {
            Err(io::ErrorKind::Unsupported.into())
        } else {
            Err(error)
        };
    }
    // The old staging name no longer belongs to us. Do not unlink it on drop.
    staged.disable_cleanup(true);
    Ok(())
}

#[cfg(windows)]
pub(super) fn publish(staged: NamedTempFile, destination: &Path) -> io::Result<()> {
    // tempfile uses MoveFileExW without a replace or copy flag on Windows.
    staged
        .persist_noclobber(destination)
        .map(|_| ())
        .map_err(|error| error.error)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(super) fn publish(_staged: NamedTempFile, _destination: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}
