use super::{DisplacedIdentity, Identity, ReplacementOutcome, ReplacementRevision, Revision};
use crate::{finder_tags, io, validate_write_size};
use preset_documents::Error;
use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io::{self as sys, Write},
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::Path,
};

const RECOVERY_ENTRY: &CStr = c"displaced-preset";
const RECOVERY_PREFIX: &str = ".preset-recovery-";
// Darwin's <sys/stdio.h> flag, not exposed by the pinned libc.
const RENAME_NOFOLLOW_ANY: libc::c_uint = 0x10;

#[cfg(test)]
mod tests;

struct Parent<'a> {
    path: &'a Path,
    name: CString,
    file: File,
    identity: Identity,
}

impl<'a> Parent<'a> {
    fn open(path: &'a Path) -> Result<Self, Error> {
        let name = path.file_name().ok_or(Error::InvalidConfiguration)?;
        let path = path.parent().ok_or(Error::InvalidConfiguration)?;
        let name = CString::new(name.as_bytes()).map_err(|_| Error::InvalidConfiguration)?;
        let file = open_parent(path).map_err(path_error)?;
        let identity = identity(&fd_stat(&file).map_err(io)?);
        Ok(Self {
            path,
            name,
            file,
            identity,
        })
    }

    fn require_local_apfs(&self) -> Result<(), Error> {
        let mut info = MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: The live directory descriptor and writable statfs output are valid.
        if unsafe { libc::fstatfs(self.file.as_raw_fd(), info.as_mut_ptr()) } != 0 {
            return Err(io(sys::Error::last_os_error()));
        }
        // SAFETY: Successful fstatfs initialized the output.
        let info = unsafe { info.assume_init() };
        let name = info.f_fstypename.map(|character| character as u8);
        if !CStr::from_bytes_until_nul(&name).is_ok_and(|value| value.to_bytes() == b"apfs")
            || info.f_flags & libc::MNT_LOCAL as u32 == 0
        {
            return Err(Error::ApprovedReplacementUnsupported);
        }
        Ok(())
    }

    fn current_revision(&self) -> Result<Revision, Error> {
        let info = entry_stat(&self.file, &self.name).map_err(path_error)?;
        revision(&info)
    }

    fn require_current(&self, expected: &ReplacementRevision) -> Result<(), Error> {
        let path_parent = open_parent(self.path).map_err(path_error)?;
        if self.identity != expected.parent
            || identity(&fd_stat(&path_parent).map_err(io)?) != self.identity
            || self.current_revision()? != expected.file
        {
            return Err(Error::DestinationChanged);
        }
        Ok(())
    }

    fn open_original(&self, expected: &Revision) -> Result<File, Error> {
        let file = open_at(
            &self.file,
            &self.name,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY,
            0,
        )
        .map_err(path_error)?;
        if revision(&fd_stat(&file).map_err(io)?)? != *expected {
            return Err(Error::DestinationChanged);
        }
        Ok(file)
    }
}

struct Recovery<'a> {
    parent: &'a Parent<'a>,
    name: CString,
    directory: File,
    identity: Identity,
    known_stage: Option<Identity>,
    initial_path: String,
    retain: bool,
}

impl<'a> Recovery<'a> {
    fn create(parent: &'a Parent<'a>) -> Result<Self, Error> {
        for _ in 0..8 {
            let name = recovery_name();
            // SAFETY: The parent is a live directory and the name is a single NUL-terminated entry.
            if unsafe { libc::mkdirat(parent.file.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                let error = sys::Error::last_os_error();
                if error.kind() == sys::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(io(error));
            }
            let directory = open_at(
                &parent.file,
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                0,
            )
            .map_err(io)?;
            let info = fd_stat(&directory).map_err(io)?;
            // SAFETY: Reading the effective uid has no arguments or process side effects.
            let uid = unsafe { libc::geteuid() };
            if info.st_uid != uid || info.st_mode & 0o777 != 0o700 {
                return Err(Error::FileAccess {
                    kind: "InvalidRecoveryDirectory".into(),
                });
            }
            let initial_path = fd_path(&directory).ok_or_else(|| Error::FileAccess {
                kind: "RecoveryLocationUnavailable".into(),
            })?;
            return Ok(Self {
                parent,
                name,
                directory,
                identity: identity(&info),
                known_stage: None,
                initial_path,
                retain: false,
            });
        }
        Err(Error::FileAccess {
            kind: "RecoveryNameUnavailable".into(),
        })
    }

    fn stage(&mut self, bytes: &[u8], original: &File) -> Result<(), Error> {
        let mut candidate = open_at(
            &self.directory,
            RECOVERY_ENTRY,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
            0o600,
        )
        .map_err(io)?;
        self.known_stage = Some(identity(&fd_stat(&candidate).map_err(io)?));
        candidate.write_all(bytes).map_err(io)?;
        finder_tags::copy_from_file_to_staging(original, &candidate)?;
        candidate.sync_all().map_err(io)
    }

    fn names_still_bound(&self) -> bool {
        entry_stat(&self.parent.file, &self.name).is_ok_and(|info| {
            info.st_mode & libc::S_IFMT == libc::S_IFDIR && identity(&info) == self.identity
        }) && self.known_stage.as_ref().is_some_and(|expected| {
            entry_stat(&self.directory, RECOVERY_ENTRY).is_ok_and(|info| {
                info.st_mode & libc::S_IFMT == libc::S_IFREG && identity(&info) == *expected
            })
        })
    }

    fn exchange(&mut self, approved: &Revision) -> Result<ReplacementOutcome, Error> {
        // From here the recovery entry might contain somebody else's item. Disarm cleanup
        // before the syscall so errors, panics and interruptions cannot remove that item.
        self.retain = true;
        // SAFETY: Both descriptors are live directories and both names are single components.
        let status = unsafe {
            libc::renameatx_np(
                self.directory.as_raw_fd(),
                RECOVERY_ENTRY.as_ptr(),
                self.parent.file.as_raw_fd(),
                self.parent.name.as_ptr(),
                libc::RENAME_SWAP | RENAME_NOFOLLOW_ANY,
            )
        };
        if status != 0 {
            let error = sys::Error::last_os_error();
            let classified = match error.raw_os_error() {
                Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP) => Some(Error::DestinationChanged),
                Some(libc::ENOTSUP) | Some(libc::EOPNOTSUPP) => {
                    Some(Error::ApprovedReplacementUnsupported)
                }
                Some(libc::EACCES | libc::EPERM | libc::EROFS | libc::EXDEV) => Some(io(error)),
                _ => None,
            };
            if let Some(error) = classified
                && self.names_still_bound()
            {
                // A known refusal with the original staging entry still present did not exchange it.
                self.retain = false;
                return Err(error);
            }
            return Ok(ReplacementOutcome::CommitUncertain {
                recovery_directory: self.location(),
            });
        }
        let displaced_identity = match entry_stat(&self.directory, RECOVERY_ENTRY) {
            Ok(info)
                if info.st_mode & libc::S_IFMT == libc::S_IFREG
                    && identity(&info) == approved.identity =>
            {
                DisplacedIdentity::MatchesApprovedFile
            }
            Ok(_) => DisplacedIdentity::Different,
            Err(_) => DisplacedIdentity::Unverified,
        };
        Ok(ReplacementOutcome::Committed {
            recovery_directory: self.location(),
            displaced_identity,
        })
    }

    fn location(&self) -> String {
        fd_path(&self.directory).unwrap_or_else(|| self.initial_path.clone())
    }
}

impl Drop for Recovery<'_> {
    fn drop(&mut self) {
        if self.retain {
            return;
        }
        if self.names_still_bound() {
            // SAFETY: This is the exact known regular staging name in our private directory.
            unsafe { libc::unlinkat(self.directory.as_raw_fd(), RECOVERY_ENTRY.as_ptr(), 0) };
        }
        if entry_stat(&self.parent.file, &self.name).is_ok_and(|info| {
            info.st_mode & libc::S_IFMT == libc::S_IFDIR && identity(&info) == self.identity
        }) {
            // SAFETY: Only an empty, known transaction directory can be removed; never recurse.
            unsafe {
                libc::unlinkat(
                    self.parent.file.as_raw_fd(),
                    self.name.as_ptr(),
                    libc::AT_REMOVEDIR,
                )
            };
        }
    }
}

pub(super) fn capture(path: &Path) -> Result<ReplacementRevision, Error> {
    let text = valid_path(path)?;
    let parent = Parent::open(path)?;
    parent.require_local_apfs()?;
    let file = parent.current_revision()?;
    Ok(ReplacementRevision {
        path: text.into(),
        parent: parent.identity,
        file,
    })
}

pub(super) fn replace(
    path: &Path,
    expected: &ReplacementRevision,
    bytes: &[u8],
    maximum: usize,
) -> Result<ReplacementOutcome, Error> {
    replace_inner(path, expected, bytes, maximum, || {}, || {})
}

fn replace_inner(
    path: &Path,
    expected: &ReplacementRevision,
    bytes: &[u8],
    maximum: usize,
    after_staging: impl FnOnce(),
    before_exchange: impl FnOnce(),
) -> Result<ReplacementOutcome, Error> {
    validate_write_size(bytes.len(), maximum)?;
    if valid_path(path)? != expected.path {
        return Err(Error::DestinationChanged);
    }
    let parent = Parent::open(path)?;
    parent.require_current(expected)?;
    parent.require_local_apfs()?;
    let original = match parent.open_original(&expected.file) {
        Ok(file) => file,
        Err(error) => {
            parent.require_current(expected)?;
            return Err(error);
        }
    };
    let mut recovery = Recovery::create(&parent)?;
    if let Err(error) = recovery.stage(bytes, &original) {
        parent.require_current(expected)?;
        return Err(error);
    }
    after_staging();
    if !recovery.names_still_bound() {
        return Err(Error::FileAccess {
            kind: "RecoveryDirectoryChanged".into(),
        });
    }
    parent.require_current(expected)?;
    before_exchange();
    recovery.exchange(&expected.file)
}

fn valid_path(path: &Path) -> Result<&str, Error> {
    let name = path.file_name().and_then(|value| value.to_str());
    path.to_str()
        .filter(|value| {
            path.is_absolute()
                && !value.contains('\0')
                && name.is_some()
                && value.rsplit('/').next() == name
        })
        .ok_or(Error::InvalidConfiguration)
}

fn path_error(error: sys::Error) -> Error {
    match error.raw_os_error() {
        Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP) => Error::DestinationChanged,
        _ => io(error),
    }
}

fn identity(info: &libc::stat) -> Identity {
    Identity {
        device: info.st_dev,
        inode: info.st_ino,
    }
}

fn revision(info: &libc::stat) -> Result<Revision, Error> {
    if info.st_mode & libc::S_IFMT != libc::S_IFREG || info.st_size < 0 {
        return Err(Error::DestinationChanged);
    }
    Ok(Revision {
        identity: identity(info),
        size: info.st_size,
        change_seconds: info.st_ctime,
        change_nanoseconds: info.st_ctime_nsec,
        modified_seconds: info.st_mtime,
        modified_nanoseconds: info.st_mtime_nsec,
    })
}

fn open_parent(path: &Path) -> sys::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY)
        .open(path)
}

fn open_at(
    parent: &File,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> sys::Result<File> {
    // SAFETY: The live parent descriptor and NUL-terminated single name are valid. The
    // variadic mode argument must use C integer promotion, even though mode_t is smaller.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC,
            libc::c_int::from(mode),
        )
    };
    if fd < 0 {
        return Err(sys::Error::last_os_error());
    }
    // SAFETY: A successful openat returns a newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn fd_stat(file: &File) -> sys::Result<libc::stat> {
    let mut info = MaybeUninit::uninit();
    // SAFETY: The live descriptor and writable stat output are valid.
    if unsafe { libc::fstat(file.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return Err(sys::Error::last_os_error());
    }
    // SAFETY: Successful fstat initialized the output.
    Ok(unsafe { info.assume_init() })
}

fn entry_stat(parent: &File, name: &CStr) -> sys::Result<libc::stat> {
    let mut info = MaybeUninit::uninit();
    // SAFETY: The live directory descriptor, NUL-terminated name and writable output are valid.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            info.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(sys::Error::last_os_error());
    }
    // SAFETY: Successful fstatat initialized the output.
    Ok(unsafe { info.assume_init() })
}

fn fd_path(directory: &File) -> Option<String> {
    let mut bytes = [0_u8; libc::PATH_MAX as usize];
    // SAFETY: F_GETPATH accepts a buffer of at least PATH_MAX bytes and a live descriptor.
    if unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_GETPATH, bytes.as_mut_ptr()) } != 0 {
        return None;
    }
    let text = CStr::from_bytes_until_nul(&bytes).ok()?.to_str().ok()?;
    Path::new(text).is_absolute().then(|| text.to_owned())
}

fn recovery_name() -> CString {
    let mut random = [0_u8; 16];
    // SAFETY: The supplied byte array is writable for the exact length passed.
    unsafe { libc::arc4random_buf(random.as_mut_ptr().cast(), random.len()) };
    let suffix = u128::from_ne_bytes(random);
    CString::new(format!("{RECOVERY_PREFIX}{suffix:032x}")).expect("ASCII recovery name")
}
