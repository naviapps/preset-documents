use preset_documents::{Error, Format, Limits, Root, decode, encode, project_metadata_timestamp};
use preset_documents_file_io::{read, write_new};
use std::path::PathBuf;

fn main() {
    let directory = PathBuf::from(std::env::args_os().nth(1).expect("owned fixture directory"));
    assert!(directory.is_absolute());
    let timestamp = project_metadata_timestamp("1969-12-31t23:59:59.875z").unwrap();
    assert_eq!(timestamp.canonical, "1969-12-31T23:59:59.875Z");
    assert_eq!(
        (timestamp.unix_seconds, timestamp.nanoseconds),
        (-1, 875_000_000)
    );
    let format = Format {
        identifier: "com.example.external-presets".into(),
        minimum_supported_version: 1,
        current_version: 1,
    };
    let limits = Limits {
        maximum_byte_count: 4096,
        maximum_document_count: 1,
        ..Limits::default()
    };
    let payload = r#"{"integer":18446744073709551615,"text":"日本語😀"}"#;
    let input = format!(
        r#"{{"formatVersion":{{"identifier":"com.example.external-presets","version":1}},"payload":{payload}}}"#
    );
    let documents = decode(input.as_bytes(), Some(&format), &limits, Root::Document).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].payload.get(), payload);
    let encoded = encode(&documents, &limits, Root::Document).unwrap();
    let restored = decode(&encoded, Some(&format), &limits, Root::Document).unwrap();
    assert_eq!(restored[0].payload.get(), payload);
    assert!(matches!(
        decode(b"{", Some(&format), &limits, Root::Document),
        Err(Error::InvalidDocument)
    ));

    let path = directory.join("rust-consumer.preset");
    write_new(&path, &encoded, limits.maximum_byte_count).unwrap();
    assert_eq!(read(&path, limits.maximum_byte_count).unwrap(), encoded);
    assert_eq!(
        write_new(&path, b"must not replace", limits.maximum_byte_count),
        Err(Error::DestinationAlreadyExists)
    );
    assert_eq!(read(&path, limits.maximum_byte_count).unwrap(), encoded);
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    println!("Standalone Rust Git consumer passed UInt64/Unicode, typed rejection and no-clobber.");
}
