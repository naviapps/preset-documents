//! The macOS Finder tag attribute is copied as opaque metadata before publication.
use super::{destination_changed, io};
use preset_documents::Error;
use std::{
    ffi::{CStr, CString},
    fs::File,
    mem::size_of_val,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::Path,
};

const FINDER_TAGS: &CStr = c"com.apple.metadata:_kMDItemUserTags";
const MAXIMUM_TAG_BYTES: usize = 1024 * 1024;

fn supports_native_xattrs(staging: &File) -> bool {
    // SAFETY: attrlist contains only integer fields; zero leaves unrequested groups empty.
    let mut attributes: libc::attrlist = unsafe { std::mem::zeroed() };
    attributes.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    attributes.volattr = libc::ATTR_VOL_INFO | libc::ATTR_VOL_CAPABILITIES;
    // A length word followed by the four capabilities and four validity words.
    let mut buffer = [0_u32; 9];
    // SAFETY: The live staging descriptor and fixed-size request/output buffers are valid.
    let status = unsafe {
        libc::fgetattrlist(
            staging.as_raw_fd(),
            (&raw mut attributes).cast(),
            buffer.as_mut_ptr().cast(),
            size_of_val(&buffer),
            0,
        )
    };
    let index = libc::VOL_CAPABILITIES_INTERFACES;
    status == 0
        && buffer[0] as usize == size_of_val(&buffer)
        && buffer[1 + index] & buffer[5 + index] & libc::VOL_CAP_INT_EXTENDED_ATTR != 0
}

pub(super) fn copy_to_staging(path: &Path, staging: &File) -> Result<(), Error> {
    // Leave volumes without reported native support on the existing write path.
    if !supports_native_xattrs(staging) {
        return Ok(());
    }
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io(std::io::Error::new(std::io::ErrorKind::InvalidInput, error)))?;
    let tags = read_tags(|buffer, length| {
        // SAFETY: Both strings and the output buffer supplied by read_tags are valid.
        unsafe {
            libc::getxattr(
                path.as_ptr(),
                FINDER_TAGS.as_ptr(),
                buffer,
                length,
                0,
                libc::XATTR_NOFOLLOW,
            )
        }
    })?;
    write_tags(staging, tags)
}

pub(super) fn copy_from_file_to_staging(original: &File, staging: &File) -> Result<(), Error> {
    if !supports_native_xattrs(staging) {
        return Ok(());
    }
    let tags = read_tags(|buffer, length| {
        // SAFETY: The original descriptor, attribute name and read_tags buffer are valid.
        unsafe {
            libc::fgetxattr(
                original.as_raw_fd(),
                FINDER_TAGS.as_ptr(),
                buffer,
                length,
                0,
                0,
            )
        }
    })?;
    write_tags(staging, tags)
}

fn write_tags(staging: &File, tags: Option<Vec<u8>>) -> Result<(), Error> {
    let Some(bytes) = tags else {
        return Ok(());
    };
    // SAFETY: The descriptor, NUL-terminated attribute name, and byte slice are live.
    let result = unsafe {
        libc::fsetxattr(
            staging.as_raw_fd(),
            FINDER_TAGS.as_ptr(),
            bytes.as_ptr().cast(),
            bytes.len(),
            0,
            0,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io(std::io::Error::last_os_error()))
    }
}

fn read_tags(
    read_attribute: impl Fn(*mut libc::c_void, usize) -> isize,
) -> Result<Option<Vec<u8>>, Error> {
    let length = read_attribute(std::ptr::null_mut(), 0);
    if length < 0 {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ENOATTR | libc::ENOTSUP | libc::EACCES | libc::EPERM) => Ok(None),
            Some(libc::ENOENT) => Err(destination_changed()),
            _ => Err(io(error)),
        };
    }
    if length as usize > MAXIMUM_TAG_BYTES {
        return Err(Error::FileAccess {
            kind: "FinderTagsTooLarge".into(),
        });
    }
    // Keep a nonzero capacity so a zero-length attribute is read, not queried again.
    let mut bytes = vec![0_u8; (length as usize).max(1)];
    let read = read_attribute(bytes.as_mut_ptr().cast(), bytes.len());
    if read < 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(libc::ENOENT) {
            destination_changed()
        } else {
            io(error)
        });
    }
    if read as usize > bytes.len() {
        return Err(Error::FileAccess {
            kind: "FinderTagsChanged".into(),
        });
    }
    bytes.truncate(read as usize);
    Ok(Some(bytes))
}
