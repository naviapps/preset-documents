use super::*;
use std::{
    fs::{self, FileTimes},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::PathBuf,
};

const OLD: &[u8] = b"old-data";
const NEW: &[u8] = b"new-data";
const RIVAL: &[u8] = b"alt-data";
const TAG: &CStr = c"com.apple.metadata:_kMDItemUserTags";
const TAG_BYTES: &[u8] =
    b"<plist version=\"1.0\"><array><string>Owned probe</string></array></plist>";

fn fixture() -> Option<(tempfile::TempDir, PathBuf, ReplacementRevision)> {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("preset.preset");
    fs::write(&path, OLD).unwrap();
    match crate::capture_replacement(&path) {
        Ok(expected) => Some((root, path, expected)),
        Err(Error::ApprovedReplacementUnsupported) => None,
        Err(error) => panic!("Cannot capture fixture: {error:?}"),
    }
}

fn committed(outcome: ReplacementOutcome, expected: DisplacedIdentity) -> PathBuf {
    let ReplacementOutcome::Committed {
        recovery_directory,
        displaced_identity,
    } = outcome
    else {
        panic!("Fixture exchange did not commit: {outcome:?}");
    };
    assert_eq!(displaced_identity, expected);
    let recovery = PathBuf::from(recovery_directory);
    let metadata = fs::symlink_metadata(&recovery).unwrap();
    assert!(metadata.is_dir());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    assert!(
        recovery
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(RECOVERY_PREFIX)
    );
    recovery.join("displaced-preset")
}

fn tag(file: &File, set: bool) -> Vec<u8> {
    if set {
        // SAFETY: The owned descriptor and fixed attribute name/data are valid.
        let status = unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                TAG.as_ptr(),
                TAG_BYTES.as_ptr().cast(),
                TAG_BYTES.len(),
                0,
                0,
            )
        };
        assert_eq!(status, 0, "{}", sys::Error::last_os_error());
    }
    let mut buffer = [0_u8; 128];
    // SAFETY: The owned descriptor and attribute name/output buffer are valid.
    let length = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            TAG.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            0,
            0,
        )
    };
    assert_eq!(
        length,
        TAG_BYTES.len() as isize,
        "{}",
        sys::Error::last_os_error()
    );
    buffer[..length as usize].to_vec()
}

#[test]
fn approved_replacement_keeps_displaced_bytes_and_finder_tags() {
    let Some((root, path, _)) = fixture() else {
        return;
    };
    let original = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    assert_eq!(tag(&original, true), TAG_BYTES);
    let revision = capture(&path).unwrap();
    let replacement = replace(&path, &revision, NEW, 64).unwrap();
    let displaced = committed(replacement, DisplacedIdentity::MatchesApprovedFile);
    assert_eq!(
        displaced.parent().unwrap().parent().unwrap(),
        root.path().canonicalize().unwrap()
    );
    assert_eq!(fs::read(&displaced).unwrap(), OLD);
    assert_eq!(fs::read(&path).unwrap(), NEW);
    assert_eq!(tag(&File::open(displaced).unwrap(), false), TAG_BYTES);
    assert_eq!(tag(&File::open(path).unwrap(), false), TAG_BYTES);
}

#[test]
fn approved_replacement_never_recreates_or_replaces_a_moved_destination() {
    let Some((root, path, approval)) = fixture() else {
        return;
    };
    let held = root.path().join("held.preset");
    fs::rename(&path, &held).unwrap();
    let entries = || {
        let mut names: Vec<_> = fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    };

    assert_eq!(
        crate::replace_approved(&path, &approval, NEW, 64),
        Err(Error::DestinationChanged)
    );
    assert_eq!(entries(), ["held.preset"]);
    assert_eq!(fs::read(&held).unwrap(), OLD);

    fs::write(&path, RIVAL).unwrap();
    assert_eq!(
        crate::replace_approved(&path, &approval, NEW, 64),
        Err(Error::DestinationChanged)
    );
    assert_eq!(entries(), ["held.preset", "preset.preset"]);
    assert_eq!(fs::read(&held).unwrap(), OLD);
    assert_eq!(fs::read(&path).unwrap(), RIVAL);
}

#[test]
fn approved_replacement_rechecks_an_in_place_edit_before_staging() {
    let Some((root, path, approval)) = fixture() else {
        return;
    };
    // Change the size through the existing inode, even if timestamps have the same resolution.
    let edited = b"edited original";
    let inode = fs::metadata(&path).unwrap().ino();
    fs::write(&path, edited).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(
        crate::replace_approved(&path, &approval, NEW, 64),
        Err(Error::DestinationChanged)
    );
    assert_eq!(fs::read(&path).unwrap(), edited);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn approved_replacement_rejects_an_observed_stale_revision() {
    let Some((root, path, expected)) = fixture() else {
        return;
    };
    let other = root.path().join("other.preset");
    fs::write(&other, RIVAL).unwrap();
    assert_eq!(
        replace(&other, &expected, NEW, 64),
        Err(Error::DestinationChanged)
    );
    assert_eq!(fs::read(&other).unwrap(), RIVAL);

    let displaced = root.path().join("held.preset");
    assert_eq!(
        replace_inner(
            &path,
            &expected,
            NEW,
            64,
            || {
                fs::rename(&path, &displaced).unwrap();
                fs::write(&path, RIVAL).unwrap();
            },
            || panic!("observed change must prevent exchange")
        ),
        Err(Error::DestinationChanged)
    );
    assert_eq!(fs::read(&path).unwrap(), RIVAL);
    assert_eq!(fs::read(&displaced).unwrap(), OLD);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);

    fs::remove_file(&path).unwrap();
    assert_eq!(
        replace(&path, &expected, NEW, 64),
        Err(Error::DestinationChanged)
    );
    assert!(!path.exists());
    symlink(&other, &path).unwrap();
    assert_eq!(
        replace(&path, &expected, NEW, 64),
        Err(Error::DestinationChanged)
    );
    assert_eq!(fs::read_link(&path).unwrap(), other);
    assert_eq!(fs::read(&other).unwrap(), RIVAL);
}

#[test]
fn approved_replacement_does_not_create_if_destination_disappears_at_commit() {
    let Some((root, path, expected)) = fixture() else {
        return;
    };
    let original = root.path().join("held.preset");
    assert_eq!(
        replace_inner(
            &path,
            &expected,
            NEW,
            64,
            || {},
            || {
                fs::rename(&path, &original).unwrap();
            }
        ),
        Err(Error::DestinationChanged)
    );
    assert!(!path.exists());
    assert_eq!(fs::read(original).unwrap(), OLD);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn approved_replacement_keeps_every_final_gap_displacement() {
    for entry in 0..3 {
        let Some((root, path, expected)) = fixture() else {
            return;
        };
        let original = root.path().join("held.preset");
        let referent = root.path().join("referent.preset");
        fs::write(&referent, RIVAL).unwrap();
        let outcome = replace_inner(
            &path,
            &expected,
            NEW,
            64,
            || {},
            || {
                fs::rename(&path, &original).unwrap();
                match entry {
                    0 => fs::write(&path, RIVAL).unwrap(),
                    1 => {
                        fs::create_dir(&path).unwrap();
                        fs::write(path.join("sentinel"), RIVAL).unwrap();
                    }
                    _ => symlink(&referent, &path).unwrap(),
                }
            },
        );
        assert_eq!(fs::read(original).unwrap(), OLD);
        assert_eq!(fs::read(&referent).unwrap(), RIVAL);
        let prior = match outcome {
            Ok(outcome) => {
                assert_eq!(fs::read(&path).unwrap(), NEW);
                committed(outcome, DisplacedIdentity::Different)
            }
            // A volume may refuse a directory or symlink exchange instead of displacing it.
            Err(Error::DestinationChanged) if entry != 0 => {
                assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
                path
            }
            other => panic!("unexpected result for fixture {entry}: {other:?}"),
        };
        match entry {
            0 => assert_eq!(fs::read(prior).unwrap(), RIVAL),
            1 => assert_eq!(fs::read(prior.join("sentinel")).unwrap(), RIVAL),
            _ => assert_eq!(fs::read_link(prior).unwrap(), referent),
        }
    }
}

#[test]
fn approved_replacement_preserves_same_inode_edits_in_final_gap() {
    let Some((_root, path, expected)) = fixture() else {
        return;
    };
    let modification_time = fs::metadata(&path).unwrap().modified().unwrap();
    let outcome = replace_inner(
        &path,
        &expected,
        NEW,
        64,
        || {},
        || {
            fs::write(&path, RIVAL).unwrap();
            File::open(&path)
                .unwrap()
                .set_times(FileTimes::new().set_modified(modification_time))
                .unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().modified().unwrap(),
                modification_time
            );
        },
    )
    .unwrap();
    let displaced = committed(outcome, DisplacedIdentity::MatchesApprovedFile);
    assert_eq!(fs::read(&displaced).unwrap(), RIVAL);
    assert_eq!(fs::read(path).unwrap(), NEW);
}
