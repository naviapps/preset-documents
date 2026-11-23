#![no_main]

use libfuzzer_sys::fuzz_target;
use preset_documents::{Format, Limits, Root, decode, normalize_document_envelopes};

fuzz_target!(|input: &[u8]| {
    let format = Format {
        identifier: "com.example.presets".into(),
        minimum_supported_version: 1,
        current_version: 2,
    };
    let limits = Limits {
        maximum_byte_count: 4096,
        maximum_document_count: 8,
        ..Limits::default()
    };
    for root in [Root::Document, Root::Collection] {
        let Ok(normalized) = normalize_document_envelopes(input, &format, &limits, root) else {
            continue;
        };
        assert!(normalized.len() <= limits.maximum_byte_count);
        let original = decode(input, Some(&format), &limits, root).unwrap();
        let round_tripped = decode(&normalized, Some(&format), &limits, root).unwrap();
        assert!(
            original
                .iter()
                .map(|document| document.payload.get())
                .eq(round_tripped.iter().map(|document| document.payload.get())),
            "normalizing the envelope changed the payload lexemes"
        );
        assert_eq!(
            normalize_document_envelopes(&normalized, &format, &limits, root).unwrap(),
            normalized
        );
    }
});
