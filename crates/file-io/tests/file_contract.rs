use preset_documents::{Error, MAXIMUM_BYTES};
use preset_documents_file_io::{read, write_atomically, write_new};
use std::{fs, path::Path};

#[test]
fn rejected_write_sizes_keep_existing_and_absent_destinations_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("existing.preset");
    let missing = root.path().join("missing.preset");
    fs::write(&existing, b"previous preset").unwrap();
    let oversized = vec![0; MAXIMUM_BYTES + 1];

    type Write = fn(&Path, &[u8], usize) -> Result<(), Error>;
    for write in [write_new as Write, write_atomically] {
        for path in [&existing, &missing] {
            for maximum in [0, MAXIMUM_BYTES + 1] {
                assert_eq!(
                    write(path, b"new", maximum),
                    Err(Error::InvalidConfiguration)
                );
            }
            for payload in [b"new".as_slice(), oversized.as_slice()] {
                assert_eq!(
                    write(path, payload, 2),
                    Err(Error::OutputTooLarge {
                        byte_count: payload.len(),
                        maximum_byte_count: 2,
                    })
                );
            }
            assert_eq!(
                write(path, &oversized, MAXIMUM_BYTES),
                Err(Error::OutputTooLarge {
                    byte_count: oversized.len(),
                    maximum_byte_count: MAXIMUM_BYTES,
                })
            );
        }
        assert_eq!(fs::read(&existing).unwrap(), b"previous preset");
        assert!(!missing.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[test]
fn bounded_read_reports_the_first_excess_byte() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preset");
    for size in [0, 1, 3, 8191, 8192, 8193, 128 * 1024] {
        let payload = vec![42; size];
        fs::write(&file, &payload).unwrap();
        for maximum in [size.saturating_sub(1).max(1), size.max(1), size + 1] {
            let expected = if size > maximum {
                Err(Error::InputTooLarge {
                    byte_count: maximum + 1,
                    maximum_byte_count: maximum,
                })
            } else {
                Ok(payload.clone())
            };
            assert_eq!(
                read(&file, maximum),
                expected,
                "size {size}, limit {maximum}"
            );
        }
    }
    assert_eq!(
        read(&file, 3),
        Err(Error::InputTooLarge {
            byte_count: 4,
            maximum_byte_count: 3,
        })
    );
    assert_eq!(read(&file, 0), Err(Error::InvalidConfiguration));
    assert_eq!(
        read(&file, MAXIMUM_BYTES + 1),
        Err(Error::InvalidConfiguration)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
#[test]
fn publication_preserves_occupied_names_until_explicit_replacement() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("日本語.preset");
    let directory = root.path().join("occupied");
    fs::create_dir(&directory).unwrap();
    let original = vec![42; 128 * 1024];
    let replacement = b"intentional replacement";

    write_new(&file, &original, original.len()).unwrap();
    assert_eq!(fs::read(&file).unwrap(), original);
    for path in [&file, &directory] {
        assert_eq!(
            write_new(path, b"competing", 9),
            Err(Error::DestinationAlreadyExists)
        );
    }
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(
        write_atomically(&directory, replacement, replacement.len()),
        Err(Error::FileAccess {
            kind: "NotRegularFile".into(),
        })
    );
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);

    write_atomically(&file, replacement, replacement.len()).unwrap();
    assert_eq!(fs::read(&file).unwrap(), replacement);
    let absent = root.path().join("new.preset");
    write_atomically(&absent, &original, original.len()).unwrap();
    assert_eq!(fs::read(&absent).unwrap(), original);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
}

#[cfg(target_os = "macos")]
#[test]
fn atomic_replacement_preserves_only_finder_tags() {
    use std::{ffi::CStr, io, os::fd::AsRawFd};

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("preset.preset");
    let finder_tags = c"com.apple.metadata:_kMDItemUserTags";
    let unrelated = c"com.example.preset-documents.fixture";
    // A binary property list containing the single Finder tag "Review\n6".
    const TAGS: &[u8] = b"bplist00\xa1\x01XReview\n6\x08\x0a\
        \0\0\0\0\0\0\x01\x01\0\0\0\0\0\0\0\x02\
        \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x13";

    write_atomically(&path, b"old", 3).unwrap();
    let original = fs::File::options()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    for (name, value) in [(finder_tags, TAGS), (unrelated, b"other")] {
        // SAFETY: The owned file, NUL-terminated name and byte slice are live.
        let result = unsafe {
            libc::fsetxattr(
                original.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert_eq!(result, 0, "{name:?}: {}", io::Error::last_os_error());
    }
    drop(original);

    write_atomically(&path, b"new", 3).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"new");
    let replacement = fs::File::open(&path).unwrap();
    let get_attribute = |name: &CStr, buffer: &mut [u8]| {
        // SAFETY: The owned file, NUL-terminated name and output buffer are live.
        unsafe {
            libc::fgetxattr(
                replacement.as_raw_fd(),
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
                0,
            )
        }
    };
    let mut actual = [0; TAGS.len()];
    let count = get_attribute(finder_tags, &mut actual);
    assert_eq!(count, TAGS.len() as isize, "{}", io::Error::last_os_error());
    assert_eq!(&actual, TAGS);
    let count = get_attribute(unrelated, &mut actual);
    let error = io::Error::last_os_error();
    assert_eq!(count, -1);
    assert_eq!(error.raw_os_error(), Some(libc::ENOATTR));
}
