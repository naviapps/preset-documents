use super::{
    Buffer, preset_documents_buffer_data, preset_documents_buffer_free, preset_documents_process,
};
use preset_documents::MAXIMUM_BYTES;
use serde_json::json;
use std::{
    fs, io,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Reply(*mut Buffer);

impl Reply {
    fn call(operation: u32, data: &[u8], options: &[u8]) -> Self {
        // Both slices remain readable throughout the call.
        Self(unsafe {
            preset_documents_process(
                operation,
                data.as_ptr(),
                data.len(),
                options.as_ptr(),
                options.len(),
            )
        })
    }

    fn view(&self) -> (u32, &[u8]) {
        assert!(!self.0.is_null());
        let (mut length, mut status) = (usize::MAX, u32::MAX);
        // The owner lives at least as long as the returned slice.
        let bytes = unsafe { preset_documents_buffer_data(self.0, &mut length, &mut status) };
        assert!(!bytes.is_null());
        (status, unsafe { std::slice::from_raw_parts(bytes, length) })
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        unsafe { preset_documents_buffer_free(self.0) }
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("preset-file-wire-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create file fixture: {error}"),
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const INVALID: &[u8] = br#"{"code":"invalidConfiguration"}"#;

#[test]
fn collection_codec_replies_preserve_count_and_whole_input_error_order() {
    const DOCUMENT: &str = r#"{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{},"payload":["日本語😀",9007199254740993,"\u0061\/"]}"#;
    const TOO_MANY: &[u8] =
        br#"{"code":"tooManyDocuments","documentCount":2,"maximumDocumentCount":1}"#;
    const INVALID_DOCUMENT: &[u8] = br#"{"code":"invalidDocument"}"#;
    const TOO_DEEP: &[u8] = br#"{"code":"nestingTooDeep","maximumDepth":3}"#;

    let options = |maximum_bytes, maximum_depth| {
        serde_json::to_vec(&json!({
            "format": {
                "identifier": "com.example.presets",
                "minimumSupportedVersion": 1,
                "currentVersion": 2
            },
            "limits": {
                "maximumByteCount": maximum_bytes,
                "maximumDocumentCount": 1,
                "maximumDepth": maximum_depth
            },
            "root": "collection"
        }))
        .unwrap()
    };
    let ordinary = options(4096, 128);
    let valid = format!("[{DOCUMENT}]");
    let compact = valid.replace(r"\u0061\/", "a/");
    let over_count = format!("[{DOCUMENT},null,{{}}]");
    let compact_count = compact.len() + ",null,{}".len();
    let byte_limit = options(over_count.len() - 1, 128);
    let tight_byte_limit = options(compact_count - 1, 128);
    let byte_error = |count, maximum| {
        format!(r#"{{"code":"inputTooLarge","byteCount":{count},"maximumByteCount":{maximum}}}"#)
    };
    let raw_limit_error = byte_error(over_count.len(), over_count.len() - 1);
    let unicode = format!(r#"[{DOCUMENT},"\ud800"]"#);
    let depth = format!("[{DOCUMENT},[[[null]]]]");
    let depth_limit = options(4096, 3);
    for operation in [2, 3, 11] {
        let typed = operation == 11;
        let expected = if typed { &compact } else { &valid };
        let tight_limit_error = byte_error(
            if typed {
                compact_count
            } else {
                over_count.len()
            },
            compact_count - 1,
        );
        for (input, options, status, expected) in [
            (valid.as_str(), &ordinary, 0, expected.as_bytes()),
            (&over_count, &ordinary, 1, TOO_MANY),
            (
                &over_count,
                &byte_limit,
                1,
                if typed {
                    TOO_MANY
                } else {
                    raw_limit_error.as_bytes()
                },
            ),
            (
                &over_count,
                &tight_byte_limit,
                1,
                tight_limit_error.as_bytes(),
            ),
            (&unicode, &ordinary, 1, INVALID_DOCUMENT),
            (&depth, &depth_limit, 1, TOO_DEEP),
        ] {
            let reply = Reply::call(operation, input.as_bytes(), options);
            assert_eq!(
                reply.view(),
                (status, expected),
                "operation {operation}, {input}"
            );
        }
    }
}

fn options(path: &Path, maximum: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({"path": path, "maximumByteCount": maximum})).unwrap()
}

#[test]
fn approval_rejects_malformed_wire_arguments_without_touching_the_fixture() {
    let fixture = Fixture::new();
    let path = fixture.0.join("日本語.preset");
    let capture = json!({"path": path}).to_string();
    let replace = json!({"path": path, "maximumByteCount": 10, "revision": []}).to_string();
    for (operation, data, arguments) in [
        (8, b"".as_slice(), r#"["file.preset"]"#.to_owned()),
        (8, b"", r#"{"path":"file.preset"}"#.to_owned()),
        (8, b"unexpected input", capture.clone()),
        (8, b"", json!({"path": path, "extra": true}).to_string()),
        (
            8,
            b"",
            json!({"path": format!("{}\0", path.display())}).to_string(),
        ),
        (9, b"input", replace),
        (
            9,
            b"",
            json!({"path": path, "maximumByteCount": 10, "revision": null}).to_string(),
        ),
    ] {
        assert_eq!(
            Reply::call(operation, data, arguments.as_bytes()).view(),
            (1, INVALID)
        );
    }
    #[cfg(not(target_os = "macos"))]
    assert_eq!(
        Reply::call(8, b"", capture.as_bytes()).view(),
        (
            1,
            br#"{"code":"approvedReplacementUnsupported"}"#.as_slice()
        )
    );
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn write_preflight_reports_submitted_size_and_checks_configuration_first() {
    for (size, maximum) in [(0, 3), (3, 3), (MAXIMUM_BYTES, MAXIMUM_BYTES)] {
        let budget = json!({"byteCount": size, "maximumByteCount": maximum}).to_string();
        assert_eq!(
            Reply::call(10, b"", budget.as_bytes()).view(),
            (0, b"".as_slice())
        );
    }
    for (size, maximum) in [
        (4, 3),
        (MAXIMUM_BYTES + 1, 3),
        (MAXIMUM_BYTES + 1, MAXIMUM_BYTES),
    ] {
        let budget = json!({"byteCount": size, "maximumByteCount": maximum}).to_string();
        let expected = format!(
            r#"{{"code":"outputTooLarge","byteCount":{size},"maximumByteCount":{maximum}}}"#
        );
        assert_eq!(
            Reply::call(10, b"", budget.as_bytes()).view(),
            (1, expected.as_bytes())
        );
    }
    for arguments in [
        br#"{"byteCount":0,"maximumByteCount":0}"#.as_slice(),
        br#"{"byteCount":0,"maximumByteCount":67108865}"#,
        br#"{"byteCount":3,"byteCount":0,"maximumByteCount":3}"#,
        br#"{"byteCount":-1,"maximumByteCount":3}"#,
        br#"[3,3]"#,
    ] {
        assert_eq!(Reply::call(10, b"", arguments).view(), (1, INVALID));
    }
    assert_eq!(
        Reply::call(10, b"input", br#"{"byteCount":0,"maximumByteCount":3}"#).view(),
        (1, INVALID)
    );
}

#[test]
fn file_replies_keep_their_bytes_across_rejected_and_committed_writes() {
    let fixture = Fixture::new();
    let file = fixture.0.join("日本語.preset");
    let missing = fixture.0.join("missing.preset");
    let original = b"\0\xffold";
    fs::write(&file, original).unwrap();
    let file_options = options(&file, original.len());
    let owner = Reply::call(5, b"", &file_options);
    assert_eq!(owner.view(), (0, original.as_slice()));
    let copied = owner.view().1.to_vec();
    assert_eq!(
        Reply::call(5, b"", &options(&file, 2)).view(),
        (
            1,
            br#"{"code":"inputTooLarge","byteCount":3,"maximumByteCount":2}"#.as_slice()
        )
    );

    for operation in [6, 7] {
        for destination in [&file, &missing] {
            assert_eq!(
                Reply::call(operation, b"new", &options(destination, 0)).view(),
                (1, INVALID)
            );
            assert_eq!(
                Reply::call(operation, b"new", &options(destination, 2)).view(),
                (
                    1,
                    br#"{"code":"outputTooLarge","byteCount":3,"maximumByteCount":2}"#.as_slice()
                )
            );
        }
    }
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    assert!(!missing.exists());

    assert_eq!(
        Reply::call(7, b"new", &options(&missing, 3)).view(),
        (0, b"".as_slice())
    );
    assert_eq!(
        Reply::call(7, b"old", &options(&missing, 3)).view(),
        (1, br#"{"code":"destinationAlreadyExists"}"#.as_slice())
    );
    assert_eq!(fs::read(&missing).unwrap(), b"new");
    assert_eq!(
        Reply::call(6, b"new", &file_options).view(),
        (0, b"".as_slice())
    );
    assert_eq!(
        Reply::call(5, b"", &file_options).view(),
        (0, b"new".as_slice())
    );
    fs::remove_file(file).unwrap();
    assert_eq!(owner.view(), (0, original.as_slice()));
    drop(owner);
    assert_eq!(copied, original);
}

#[test]
fn transport_limits_and_invalid_views_preserve_the_native_contract() {
    // Null with nonzero length is rejected without borrowing a foreign view.
    let invalid = Reply(unsafe { preset_documents_process(2, null(), 1, null(), 0) });
    assert_eq!(invalid.view(), (1, INVALID));
    let oversized =
        Reply(unsafe { preset_documents_process(2, null(), MAXIMUM_BYTES + 1, null(), 0) });
    assert_eq!(
        oversized.view(),
        (
            1,
            br#"{"code":"inputTooLarge","byteCount":67108865,"maximumByteCount":67108864}"#
                .as_slice()
        )
    );
    let mut options = br#"{"byteCount":0,"maximumByteCount":3}"#.to_vec();
    options.resize(16384, b' ');
    assert_eq!(Reply::call(10, b"", &options).view(), (0, b"".as_slice()));
    options.push(b' ');
    assert_eq!(Reply::call(10, b"", &options).view(), (1, INVALID));

    let (mut length, mut status) = (8193, 99);
    unsafe {
        assert!(preset_documents_buffer_data(null(), &mut length, &mut status).is_null());
        assert!(preset_documents_buffer_data(invalid.0, null_mut(), &mut status).is_null());
        assert!(preset_documents_buffer_data(invalid.0, &mut length, null_mut()).is_null());
        preset_documents_buffer_free(null_mut());
    }
    assert_eq!((length, status), (8193, 99));
    assert_eq!(invalid.view(), (1, INVALID));
}
