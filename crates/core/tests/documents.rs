use preset_documents::*;

fn format() -> Format {
    Format {
        identifier: "com.example.presets".into(),
        minimum_supported_version: 1,
        current_version: 2,
    }
}
const DOCUMENT: &[u8] = br#"{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{"title":"Example","createdAt":"2026-10-30T12:34:56.1234567+09:00"},"payload":{"integer":9007199254740993,"decimal":1.234567890123456789,"escaped":"[\\\"{"}}"#;

fn codec_outputs(
    input: &[u8],
    format: &Format,
    limits: &Limits,
    root: Root,
) -> [(&'static str, Result<Vec<u8>, Error>); 3] {
    [
        (
            "normalize envelopes",
            normalize_document_envelopes(input, format, limits, root),
        ),
        (
            "decode and encode",
            decode(input, Some(format), limits, root)
                .and_then(|documents| encode(&documents, limits, root)),
        ),
        (
            "encode typed documents",
            encode_typed_documents(input, Some(format), limits, root),
        ),
    ]
}

#[test]
fn codec_modes_preserve_or_compact_payload_lexemes() {
    let payload = r#"{"z": 1.2300e+04, "a":9007199254740993,"escaped":"\u0061\/","nested": [ 2, 1 ],"dup":1,"dup":2}"#;
    let input = format!(
        r#"{{ "payload": {payload}, "metadata": null, "future": true,
            "formatVersion": {{ "version": 1, "identifier": "com.example.presets" }} }}"#
    );
    let expected = format!(
        r#"{{"formatVersion":{{"identifier":"com.example.presets","version":1}},"metadata":{{}},"payload":{payload}}}"#
    );
    for root in [Root::Document, Root::Collection] {
        let (input, expected, wrong_root) = match root {
            Root::Document => (input.clone(), expected.clone(), Root::Collection),
            Root::Collection => (
                format!("[ {input} ]"),
                format!("[{expected}]"),
                Root::Document,
            ),
        };
        let format = format();
        let limits = Limits::default();
        let expected_typed = expected.replace(r"\u0061\/", "a/");
        for ((codec, actual), expected) in codec_outputs(input.as_bytes(), &format, &limits, root)
            .into_iter()
            .zip([&expected, &expected, &expected_typed])
        {
            let output = actual.unwrap();
            assert_eq!(output, expected.as_bytes(), "{codec}, {root:?}");
            assert_eq!(
                normalize_document_envelopes(&output, &format, &limits, root).unwrap(),
                output,
                "{codec}, {root:?}",
            );
        }
        for (codec, actual) in codec_outputs(input.as_bytes(), &format, &limits, wrong_root) {
            assert_eq!(
                actual,
                Err(Error::InvalidDocument),
                "{codec}, {wrong_root:?}"
            );
        }
    }
}

#[test]
fn typed_string_escapes_measure_compact_utf8_without_rewriting_other_lexemes() {
    let escaped = r#"{"\u0061": 1.2300e+04, "a":9007199254740993,"text":"\u003C\u96EA\u003E\u0026\u0027\u002B\u0060\/\uD83D\uDE00\uD840\uDC00\u00A0\u2028\uE000\u0022\u005C\u000A\u001F"}"#;
    let compact = concat!(
        r#"{"a": 1.2300e+04, "a":9007199254740993,"text":"<雪>&'+`/😀𠀀"#,
        "\u{00a0}\u{2028}\u{e000}",
        r#"\"\\\n\u001f"}"#
    );
    for root in [Root::Document, Root::Collection] {
        let document = |payload: &str| {
            let document = format!(
                r#"{{"formatVersion":{{"identifier":"com.example.presets","version":1}},"metadata":{{}},"payload":{payload}}}"#
            );
            match root {
                Root::Document => document,
                Root::Collection => format!("[{document}]"),
            }
        };
        let input = document(escaped);
        let expected = document(compact);
        let limits = Limits {
            maximum_byte_count: expected.len(),
            ..Limits::default()
        };
        for transport in [input.as_str(), expected.as_str()] {
            assert_eq!(
                encode_typed_documents(transport.as_bytes(), None, &limits, root).unwrap(),
                expected.as_bytes()
            );
            let maximum_byte_count = limits.maximum_byte_count - 1;
            assert_eq!(
                encode_typed_documents(
                    transport.as_bytes(),
                    None,
                    &Limits {
                        maximum_byte_count,
                        ..limits.clone()
                    },
                    root,
                ),
                Err(Error::InputTooLarge {
                    byte_count: expected.len(),
                    maximum_byte_count,
                })
            );
        }
        assert_eq!(
            normalize_document_envelopes(input.as_bytes(), &format(), &Limits::default(), root)
                .unwrap(),
            input.as_bytes()
        );
    }
}

#[test]
fn slash_rich_typed_exports_use_the_compact_four_mib_budget() {
    const MAXIMUM: usize = 4 * 1024 * 1024;
    const PREFIX: &str = concat!(
        r#"[{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{},"payload":"#,
        r#"{"label":"quoted \"folder\" \\ name\n日本語😀","urls":["#,
    );
    const SUFFIX: &str = "]}}]";
    const URL_PREFIX: &str = "https://example.invalid/";
    let repeated_url = format!("{URL_PREFIX}{}index", "folder/".repeat(64));
    let repeated_entry = format!("\"{repeated_url}\",");
    let remaining = MAXIMUM - PREFIX.len() - SUFFIX.len() - URL_PREFIX.len() - 2;
    let final_url = format!(
        "{URL_PREFIX}{}",
        "x".repeat(remaining % repeated_entry.len())
    );
    let repeated_count = remaining / repeated_entry.len();
    let expected = format!(
        "{PREFIX}{}\"{final_url}\"{SUFFIX}",
        repeated_entry.repeat(repeated_count),
    );
    assert_eq!(expected.len(), MAXIMUM);

    let limits = Limits {
        maximum_byte_count: MAXIMUM,
        ..Limits::default()
    };
    for transport in [
        expected.replace('/', r"\/"),
        expected
            .replace("日本語😀", r"\u65E5\u672C\u8A9E\uD83D\uDE00")
            .replace(r#"\""#, r"\u0022"),
    ] {
        assert!(transport.len() > MAXIMUM);
        let encoded =
            encode_typed_documents(transport.as_bytes(), None, &limits, Root::Collection).unwrap();
        assert_eq!(encoded, expected.as_bytes());
        assert_eq!(
            encode_typed_documents(
                transport.as_bytes(),
                None,
                &Limits {
                    maximum_byte_count: MAXIMUM - 1,
                    ..limits.clone()
                },
                Root::Collection,
            ),
            Err(Error::InputTooLarge {
                byte_count: MAXIMUM,
                maximum_byte_count: MAXIMUM - 1,
            }),
        );
    }
    let restored = decode(
        expected.as_bytes(),
        Some(&format()),
        &limits,
        Root::Collection,
    )
    .unwrap();
    let payload: serde_json::Value = serde_json::from_str(restored[0].payload.get()).unwrap();
    assert_eq!(payload["label"], "quoted \"folder\" \\ name\n日本語😀");
    let urls = payload["urls"].as_array().unwrap();
    assert_eq!(urls.len(), repeated_count + 1);
    assert!(
        urls[..repeated_count]
            .iter()
            .all(|url| *url == repeated_url)
    );
    assert_eq!(urls[repeated_count], final_url);
}

#[test]
fn typed_writes_retain_literal_backslashes_and_surrounding_short_escapes() {
    let document = |payload: &str| {
        format!(
            r#"{{"formatVersion":{{"identifier":"com.example.presets","version":1}},"metadata":{{}},"payload":{payload}}}"#
        )
    };
    let input = document(
        r#"{"\\u0061":"\\/","short":"C:\\Temp\n\r\f\b\t\"","rewrite":"\u96EA\/\uD83D\uDE00"}"#,
    );
    let expected =
        document(r#"{"\\u0061":"\\/","short":"C:\\Temp\n\r\f\b\t\"","rewrite":"雪/😀"}"#);
    let limits = Limits {
        maximum_byte_count: expected.len(),
        ..Limits::default()
    };
    assert_eq!(
        encode_typed_documents(input.as_bytes(), None, &limits, Root::Document).unwrap(),
        expected.as_bytes()
    );
}

#[test]
fn typed_writes_preserve_limit_order_for_compact_and_malformed_strings() {
    let escaped = br#"[[[["\u003C"]]]]"#;
    let compact = br#"[[[["<"]]]]"#;
    let malformed = br#"[[[["\uD800"]]]]"#;
    let malformed_short_escape = br#"[[[["\q"]]]]"#;
    let unescaped_control = b"[[[[\"\x1f\"]]]]";
    let incomplete = br#"[[[["\u003C"#;
    for (transport, byte_count) in [
        (escaped.as_slice(), compact.len()),
        (malformed.as_slice(), malformed.len()),
        (
            malformed_short_escape.as_slice(),
            malformed_short_escape.len(),
        ),
        (unescaped_control.as_slice(), unescaped_control.len()),
        (incomplete.as_slice(), incomplete.len()),
    ] {
        let mut limits = Limits {
            maximum_byte_count: byte_count - 1,
            maximum_depth: 3,
            ..Limits::default()
        };
        let encode =
            |limits: &Limits| encode_typed_documents(transport, None, limits, Root::Collection);
        assert_eq!(
            encode(&limits),
            Err(Error::InputTooLarge {
                byte_count,
                maximum_byte_count: byte_count - 1,
            })
        );
        limits.maximum_byte_count = byte_count;
        assert_eq!(
            encode(&limits),
            Err(Error::NestingTooDeep { maximum_depth: 3 })
        );
        limits.maximum_depth = 4;
        assert_eq!(encode(&limits), Err(Error::InvalidDocument));
    }
}

#[test]
fn round_trip_preserves_opaque_payload_numbers_and_shared_metadata() {
    let docs = decode(
        DOCUMENT,
        Some(&format()),
        &Limits::default(),
        Root::Document,
    )
    .unwrap();
    let payload = docs[0].payload.get().to_owned();
    assert!(payload.contains("9007199254740993"));
    assert!(payload.contains("1.234567890123456789"));
    let bytes = encode(&docs, &Limits::default(), Root::Document).unwrap();
    let again = decode(&bytes, Some(&format()), &Limits::default(), Root::Document).unwrap();
    assert_eq!(again[0].payload.get(), payload);
    assert_eq!(again[0].metadata.created_at, docs[0].metadata.created_at);
}

fn timestamp_document(timestamp: &str) -> String {
    std::str::from_utf8(DOCUMENT).unwrap().replace(
        "\"2026-10-30T12:34:56.1234567+09:00\"",
        &serde_json::to_string(timestamp).unwrap(),
    )
}

#[derive(serde::Deserialize)]
struct TimestampCase {
    input: String,
    canonical: String,
}

#[derive(serde::Deserialize)]
struct TimestampCorpus {
    accepted: Vec<TimestampCase>,
    rejected: Vec<String>,
}

fn timestamp_corpus() -> TimestampCorpus {
    serde_json::from_str(include_str!(
        "../../../fixtures/documents/rfc3339-timestamps.json"
    ))
    .unwrap()
}

#[test]
fn rfc3339_metadata_round_trips_in_documents_and_collections() {
    let cases = timestamp_corpus().accepted;
    assert!(!cases.is_empty());
    let verify = |input: &str, expected: &str, root| {
        let limits = Limits::default();
        let normalized =
            normalize_document_envelopes(input.as_bytes(), &format(), &limits, root).unwrap();
        assert_eq!(normalized, expected.as_bytes(), "{input}");
        assert_eq!(
            encode_typed_documents(input.as_bytes(), None, &limits, root).unwrap(),
            expected.as_bytes(),
            "{input}"
        );
        let decoded = decode(&normalized, Some(&format()), &limits, root).unwrap();
        assert_eq!(encode(&decoded, &limits, root).unwrap(), normalized);
    };
    let mut input_collection = Vec::new();
    let mut canonical_collection = Vec::new();
    for case in cases {
        let projection = project_metadata_timestamp(&case.input).unwrap();
        assert_eq!(projection.canonical, case.canonical, "{}", case.input);
        let input = timestamp_document(&case.input);
        let canonical = timestamp_document(&case.canonical);
        verify(&input, &canonical, Root::Document);
        input_collection.push(input);
        canonical_collection.push(canonical);
    }
    verify(
        &format!("[{}]", input_collection.join(",")),
        &format!("[{}]", canonical_collection.join(",")),
        Root::Collection,
    );
}

#[test]
fn timestamp_projection_returns_floor_seconds_and_preserves_precision_evidence() {
    let projection = project_metadata_timestamp("1970-01-01 00:00:00.0000001001+00:30").unwrap();
    assert_eq!(projection.unix_seconds, -1800);
    assert_eq!(projection.nanoseconds, 100);
    assert_eq!(projection.offset_seconds, 1800);
    assert!(projection.has_subnanosecond_precision);
    let exact = project_metadata_timestamp("1970-01-01t00:00:00.0000001000z").unwrap();
    assert_eq!(exact.unix_seconds, 0);
    assert_eq!(exact.nanoseconds, 100);
    assert_eq!(exact.offset_seconds, 0);
    assert!(!exact.has_subnanosecond_precision);
}

#[test]
fn invalid_rfc3339_metadata_is_never_coerced_into_a_date() {
    let rejected = timestamp_corpus().rejected;
    assert!(!rejected.is_empty());
    for timestamp in rejected {
        assert!(
            matches!(
                project_metadata_timestamp(&timestamp),
                Err(Error::InvalidDocument)
            ),
            "{timestamp:?}"
        );
        let document = timestamp_document(&timestamp);
        let collection = format!("[{document}]");
        let format = format();
        let limits = Limits::default();
        for (input, root) in [(document, Root::Document), (collection, Root::Collection)] {
            assert_eq!(
                normalize_document_envelopes(input.as_bytes(), &format, &limits, root),
                Err(Error::InvalidDocument),
                "{timestamp:?}"
            );
            assert_eq!(
                encode_typed_documents(input.as_bytes(), None, &limits, root),
                Err(Error::InvalidDocument),
                "{timestamp:?}"
            );
            assert!(
                matches!(
                    decode(input.as_bytes(), Some(&format), &limits, root),
                    Err(Error::InvalidDocument)
                ),
                "{timestamp:?}"
            );
        }
    }
}

fn saved_unicode_variants(json: &str) -> Vec<Vec<u8>> {
    let mut variants = vec![
        json.as_bytes().to_vec(),
        [vec![0xef, 0xbb, 0xbf], json.as_bytes().to_vec()].concat(),
    ];
    for little in [true, false] {
        let utf16: Vec<u8> = json
            .encode_utf16()
            .flat_map(|u| {
                if little {
                    u.to_le_bytes()
                } else {
                    u.to_be_bytes()
                }
            })
            .collect();
        variants.push(utf16.clone());
        variants.push(
            [
                if little {
                    vec![0xff, 0xfe]
                } else {
                    vec![0xfe, 0xff]
                },
                utf16,
            ]
            .concat(),
        );
        let utf32: Vec<u8> = json
            .chars()
            .flat_map(|u| {
                if little {
                    (u as u32).to_le_bytes()
                } else {
                    (u as u32).to_be_bytes()
                }
            })
            .collect();
        variants.push(utf32.clone());
        variants.push(
            [
                if little {
                    vec![0xff, 0xfe, 0, 0]
                } else {
                    vec![0, 0, 0xfe, 0xff]
                },
                utf32,
            ]
            .concat(),
        );
    }
    variants
}

#[test]
fn saved_unicode_encodings_use_the_original_byte_budget_in_both_roots() {
    let document = std::str::from_utf8(DOCUMENT)
        .unwrap()
        .replace("Example", "日本語😀");
    let collection = format!("[{document}]");
    for (json, root) in [
        (document.as_str(), Root::Document),
        (collection.as_str(), Root::Collection),
    ] {
        for input in saved_unicode_variants(json) {
            let mut limits = Limits {
                maximum_byte_count: input.len(),
                ..Limits::default()
            };
            let docs = decode(&input, Some(&format()), &limits, root).unwrap();
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0].metadata.title.as_deref(), Some("日本語😀"));
            assert!(docs[0].payload.get().contains("9007199254740993"));
            assert!(docs[0].payload.get().contains("1.234567890123456789"));
            limits.maximum_byte_count -= 1;
            assert_eq!(
                decode(&input, Some(&format()), &limits, root).unwrap_err(),
                Error::InputTooLarge {
                    byte_count: input.len(),
                    maximum_byte_count: limits.maximum_byte_count,
                },
            );
        }
    }
}

#[test]
fn malformed_saved_unicode_is_rejected_in_both_roots() {
    for root in [Root::Document, Root::Collection] {
        for input in [
            &b"\xef\xbb\xbf\xf0\x80\x80\x80"[..],
            &b"\xff\xfe\x00\xd8"[..],
            &b"\xfe\xff\xdc\x00"[..],
            &b"\x00\x00\xfe\xff\x00\x11\x00\x00"[..],
            &b"\xff\xfe\x00\x00\x00\xd8\x00\x00"[..],
            &b"\xff\xfe\x00"[..],
        ] {
            assert_eq!(
                decode(input, Some(&format()), &Limits::default(), root).unwrap_err(),
                Error::InvalidDocument,
            );
        }
    }
}

#[test]
fn format_errors_precede_host_payload_and_metadata_interpretation() {
    for (from, to, code) in [
        ("com.example.presets", "com.other.presets", 0),
        ("\"version\":1", "\"version\":3", 1),
    ] {
        let input = std::str::from_utf8(DOCUMENT)
            .unwrap()
            .replace(from, to)
            .replace("2026-10-30T12:34:56.1234567+09:00", "invalid");
        let error = decode(
            input.as_bytes(),
            Some(&format()),
            &Limits::default(),
            Root::Document,
        )
        .unwrap_err();
        assert!(match code {
            0 => matches!(error, Error::FormatMismatch { .. }),
            _ => matches!(error, Error::UnsupportedFormatVersion { .. }),
        });
    }
    let no_metadata=br#"{"formatVersion":{"identifier":"com.example.presets","version":1},"payload":null,"future":true}"#;
    let docs = decode(
        no_metadata,
        Some(&format()),
        &Limits::default(),
        Root::Document,
    )
    .unwrap();
    assert_eq!(docs[0].payload.get(), "null");
    assert!(docs[0].metadata.created_at.is_none());
}

#[test]
fn malformed_ambiguous_and_wrong_root_documents_are_rejected() {
    for input in [
        "{}",
        "[]",
        "null",
        "true",
        "{\"formatVersion\":{},\"formatVersion\":{}}",
        "{} garbage",
    ] {
        assert!(matches!(
            decode(
                input.as_bytes(),
                Some(&format()),
                &Limits::default(),
                Root::Document
            ),
            Err(Error::InvalidDocument)
        ));
    }
    assert!(matches!(
        decode(
            DOCUMENT,
            Some(&format()),
            &Limits::default(),
            Root::Collection
        ),
        Err(Error::InvalidDocument)
    ));
    let invalid_date = std::str::from_utf8(DOCUMENT)
        .unwrap()
        .replace("2026-10-30T12:34:56.1234567+09:00", "2026-02-30T01:00:00Z");
    assert!(matches!(
        decode(
            invalid_date.as_bytes(),
            Some(&format()),
            &Limits::default(),
            Root::Document
        ),
        Err(Error::InvalidDocument)
    ));
}

#[test]
fn document_depth_output_and_configuration_limits_are_enforced() {
    let input = [b"[".as_slice(), DOCUMENT, b",", DOCUMENT, b"]"].concat();
    let limits = Limits {
        maximum_document_count: 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&input, Some(&format()), &limits, Root::Collection),
        Err(Error::TooManyDocuments { .. })
    ));
    let docs = decode(
        DOCUMENT,
        Some(&format()),
        &Limits::default(),
        Root::Document,
    )
    .unwrap();
    let byte_count = encode(&docs, &Limits::default(), Root::Document)
        .unwrap()
        .len();
    let mut limits = Limits {
        maximum_byte_count: byte_count,
        ..Limits::default()
    };
    assert_eq!(
        encode(&docs, &limits, Root::Document).unwrap().len(),
        byte_count
    );
    limits.maximum_byte_count -= 1;
    assert_eq!(
        encode(&docs, &limits, Root::Document),
        Err(Error::OutputTooLarge {
            byte_count,
            maximum_byte_count: byte_count - 1,
        })
    );
    assert!(matches!(
        decode(
            DOCUMENT,
            Some(&format()),
            &Limits {
                maximum_depth: 1,
                ..Limits::default()
            },
            Root::Document
        ),
        Err(Error::NestingTooDeep { .. })
    ));
    for identifier in ["", ".bad", "bad..id", "bad/id", "日本語"] {
        assert_eq!(
            Format {
                identifier: identifier.into(),
                ..format()
            }
            .validate(),
            Err(Error::InvalidConfiguration)
        );
    }
    let docs = vec![Document {
        format_version: FormatVersion {
            identifier: "invalid/id".into(),
            version: 1,
        },
        metadata: Metadata::default(),
        payload: serde_json::from_str("null").unwrap(),
    }];
    assert!(matches!(
        encode(&docs, &Limits::default(), Root::Document),
        Err(Error::InvalidDocument)
    ));
}

#[test]
fn configuration_limits_are_checked_before_payload_and_collection_count() {
    let check = |json: &str| {
        serde_json::from_str::<Limits>(json)
            .map_err(|_| Error::InvalidConfiguration)?
            .validate()
    };
    let config = |bytes, count, depth| {
        format!(
            r#"{{"maximumByteCount":{bytes},"maximumDocumentCount":{count},"maximumDepth":{depth}}}"#
        )
    };
    let maximum_bytes = 64 * 1024 * 1024;
    let format = format();
    for json in [config(1, 1, 1), config(maximum_bytes, 1, 128)] {
        assert_eq!(check(&json), Ok(()), "{json}");
    }
    for json in [
        config(0, 1, 128),
        config(maximum_bytes + 1, 1, 128),
        config(4096, 0, 128),
        config(4096, 1, 0),
        config(4096, 1, 129),
        config(4096, 1, i64::from(i32::MAX)),
    ] {
        let limits = serde_json::from_str::<Limits>(&json).unwrap();
        assert_eq!(
            limits.validate(),
            Err(Error::InvalidConfiguration),
            "{json}"
        );
        assert_eq!(
            limits.validate_document_count(2),
            Err(Error::InvalidConfiguration),
            "{json} before an excess collection count"
        );
        for root in [Root::Document, Root::Collection] {
            assert_eq!(
                normalize_document_envelopes(b"?", &format, &limits, root),
                Err(Error::InvalidConfiguration),
                "{json} before malformed saved {root:?}"
            );
            assert_eq!(
                encode_typed_documents(b"?", None, &limits, root),
                Err(Error::InvalidConfiguration),
                "{json} before malformed typed {root:?}"
            );
        }
    }
    for json in [config(4096, 1, -1), config(4096, 1, i64::MAX), "{}".into()] {
        assert_eq!(check(&json), Err(Error::InvalidConfiguration), "{json}");
    }
}

fn nested_preset(depth: usize, root: Root) -> String {
    let envelope_depth = match root {
        Root::Document => 1,
        Root::Collection => 2,
    };
    let wrappers = depth - envelope_depth - 1;
    let document = format!(
        r#"{{"formatVersion":{{"identifier":"com.example.presets","version":1}},"metadata":{{"title":"日本語😀"}},"payload":{}["日本語😀",9007199254740993,18446744073709551616,1.234567890123456789]{}}}"#,
        "[".repeat(wrappers),
        "]".repeat(wrappers),
    );
    match root {
        Root::Document => document,
        Root::Collection => format!("[{document}]"),
    }
}

#[test]
fn depth_boundaries_include_the_envelope_and_collection_root() {
    let format = format();
    for (root, minimum_depth) in [(Root::Document, 2), (Root::Collection, 3)] {
        let verify = |depth, limits: &Limits| {
            let input = nested_preset(depth, root);
            let expected = if depth > limits.maximum_depth {
                Err(Error::NestingTooDeep {
                    maximum_depth: limits.maximum_depth,
                })
            } else {
                Ok(input.as_bytes().to_vec())
            };
            assert_eq!(
                normalize_document_envelopes(input.as_bytes(), &format, limits, root),
                expected,
                "{root:?} saved input at depth {depth}"
            );
            assert_eq!(
                encode_typed_documents(input.as_bytes(), None, limits, root),
                expected,
                "{root:?} authored transport at depth {depth}"
            );
        };
        for depth in [64, 65, 96, 127, 128, 129, 192] {
            verify(depth, &Limits::default());
        }
        for maximum_depth in [minimum_depth, 4, 65, 96, 128] {
            let limits = Limits {
                maximum_depth,
                ..Limits::default()
            };
            for depth in [maximum_depth, maximum_depth + 1, maximum_depth + 32] {
                verify(depth, &limits);
            }
        }
        verify(
            4,
            &Limits {
                maximum_depth: 1,
                ..Limits::default()
            },
        );
    }
}

#[test]
fn empty_collection_uses_one_level_and_two_bytes() {
    let format = format();
    let limits = Limits {
        maximum_byte_count: 2,
        maximum_depth: 1,
        ..Limits::default()
    };
    let root = Root::Collection;
    assert_eq!(
        normalize_document_envelopes(b"[]", &format, &limits, root).unwrap(),
        b"[]"
    );
    assert_eq!(
        encode_typed_documents(b"[]", None, &limits, root).unwrap(),
        b"[]"
    );
}

#[test]
fn deepest_documents_preserve_payload_at_saved_and_typed_byte_boundaries() {
    let format = format();
    let defaults = Limits::default();
    for root in [Root::Document, Root::Collection] {
        let json = nested_preset(defaults.maximum_depth, root);
        let expected = json.as_bytes();
        let utf16: Vec<_> = json.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut limits = defaults.clone();
        for input in [expected, &utf16] {
            limits.maximum_byte_count = input.len();
            assert_eq!(
                normalize_document_envelopes(input, &format, &limits, root).unwrap(),
                expected
            );
            limits.maximum_byte_count -= 1;
            assert_eq!(
                normalize_document_envelopes(input, &format, &limits, root),
                Err(Error::InputTooLarge {
                    byte_count: input.len(),
                    maximum_byte_count: input.len() - 1,
                })
            );
        }
        let escaped = json.replace("日本語😀", r"\u65e5\u672c\u8a9e\uD83D\uDE00");
        for input in [expected, escaped.as_bytes()] {
            limits.maximum_byte_count = expected.len();
            assert_eq!(
                encode_typed_documents(input, None, &limits, root).unwrap(),
                expected
            );
            limits.maximum_byte_count -= 1;
            assert_eq!(
                encode_typed_documents(input, None, &limits, root),
                Err(Error::InputTooLarge {
                    byte_count: expected.len(),
                    maximum_byte_count: expected.len() - 1,
                })
            );
        }
    }
}

#[test]
fn normalized_bytes_use_an_independent_output_budget() {
    let format = format();
    for root in [Root::Document, Root::Collection] {
        let expected = nested_preset(65, root).replacen(r#""title":"日本語😀""#, "", 1);
        let input = expected.replacen(r#","metadata":{}"#, "", 1);
        let normalize = |bytes, maximum_byte_count| {
            normalize_document_envelopes(
                bytes,
                &format,
                &Limits {
                    maximum_byte_count,
                    maximum_depth: 65,
                    ..Limits::default()
                },
                root,
            )
        };
        for maximum_byte_count in [input.len(), expected.len() - 1] {
            assert_eq!(
                normalize(input.as_bytes(), maximum_byte_count),
                Err(Error::OutputTooLarge {
                    byte_count: expected.len(),
                    maximum_byte_count,
                })
            );
        }
        assert_eq!(
            normalize(input.as_bytes(), expected.len()).unwrap(),
            expected.as_bytes()
        );
        let utf16: Vec<_> = input.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            normalize(&utf16, utf16.len() - 1),
            Err(Error::InputTooLarge {
                byte_count: utf16.len(),
                maximum_byte_count: utf16.len() - 1,
            })
        );
        assert_eq!(normalize(&utf16, utf16.len()).unwrap(), expected.as_bytes());
    }
}

#[test]
fn object_only_envelopes_reject_positional_arrays_without_changing_count_precedence() {
    let format = format();
    let limits = Limits {
        maximum_document_count: 1,
        ..Limits::default()
    };
    for input in [
        r#"[{"identifier":"com.example.presets","version":1},null,42]"#,
        r#"{"formatVersion":["com.example.presets",1],"payload":42}"#,
        r#"{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":[null,"Title",null,null],"payload":42}"#,
    ] {
        for (bytes, root) in [
            (input.as_bytes().to_vec(), Root::Document),
            (format!("[{input}]").into_bytes(), Root::Collection),
        ] {
            for (codec, actual) in codec_outputs(&bytes, &format, &limits, root) {
                assert_eq!(
                    actual,
                    Err(Error::InvalidDocument),
                    "{codec}: {input}, {root:?}"
                );
            }
        }
        let excess = [b"[".as_slice(), DOCUMENT, b",", input.as_bytes(), b"]"].concat();
        for (codec, actual) in codec_outputs(&excess, &format, &limits, Root::Collection) {
            assert_eq!(
                actual,
                Err(Error::TooManyDocuments {
                    document_count: 2,
                    maximum_document_count: 1,
                }),
                "{codec}: {input}",
            );
        }
    }
}

#[test]
fn object_only_envelopes_preserve_opaque_array_payloads_and_metadata_validation() {
    let payload = r#"["日本語😀",9007199254740993,18446744073709551616,1.234567890123456789,{"nested":[true,null]}]"#;
    let input = format!(
        r#"{{"formatVersion":{{"identifier":"com.example.presets","version":1}},"metadata":{{"title":"日本語😀","future":true}},"payload":{payload},"future":[]}}"#
    );
    for (bytes, root) in [
        (input.as_bytes().to_vec(), Root::Document),
        (format!("[{input}]").into_bytes(), Root::Collection),
    ] {
        let documents = decode(&bytes, Some(&format()), &Limits::default(), root).unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].payload.get(), payload);
        assert_eq!(documents[0].metadata.title.as_deref(), Some("日本語😀"));
        let encoded = encode(&documents, &Limits::default(), root).unwrap();
        let restored = decode(&encoded, Some(&format()), &Limits::default(), root).unwrap();
        assert_eq!(restored[0].payload.get(), payload);
    }
    let duplicate = input.replace("\"title\":", "\"title\":null,\"title\":");
    assert_eq!(
        decode(
            duplicate.as_bytes(),
            Some(&format()),
            &Limits::default(),
            Root::Document
        )
        .unwrap_err(),
        Error::InvalidDocument,
    );
    // Metadata is still interpreted after format checks.
    let mismatch = input
        .replace("com.example.presets", "com.other.presets")
        .replace(r#"{"title":"日本語😀","future":true}"#, "[]");
    assert!(matches!(
        decode(
            mismatch.as_bytes(),
            Some(&format()),
            &Limits::default(),
            Root::Document
        ),
        Err(Error::FormatMismatch { .. }),
    ));
}

#[test]
fn collection_limit_stops_before_deserializing_the_first_excess_document() {
    let format = format();
    let limits = Limits {
        maximum_document_count: 1,
        ..Limits::default()
    };
    // An excess element must not need a valid envelope or payload. In particular,
    // it must not allocate an opaque payload before the count limit is enforced.
    for tail in [
        "null".to_owned(),
        "{}".to_owned(),
        r#"{"payload":"#.to_owned(),
        String::from_utf8([DOCUMENT, b",", DOCUMENT].concat()).unwrap(),
    ] {
        let input = [b"[".as_slice(), DOCUMENT, b",", tail.as_bytes(), b"]"].concat();
        for (codec, actual) in codec_outputs(&input, &format, &limits, Root::Collection) {
            assert_eq!(
                actual,
                Err(Error::TooManyDocuments {
                    document_count: 2,
                    maximum_document_count: 1,
                }),
                "{codec}, tail: {tail}",
            );
        }
    }
}

#[test]
fn bounded_collection_reads_validate_array_framing_and_the_accepted_prefix() {
    let limits = Limits {
        maximum_document_count: 1,
        ..Limits::default()
    };
    for (input, count) in [
        (b"[]".to_vec(), 0),
        ([b"[".as_slice(), DOCUMENT, b"]"].concat(), 1),
    ] {
        let documents = decode(&input, Some(&format()), &limits, Root::Collection).unwrap();
        assert_eq!(documents.len(), count);
        if count == 1 {
            let original = decode(DOCUMENT, Some(&format()), &limits, Root::Document).unwrap();
            assert_eq!(documents[0].payload.get(), original[0].payload.get());
        }
    }
    for input in [
        DOCUMENT.to_vec(),
        [b"[".as_slice(), DOCUMENT, b",]"].concat(),
        [b"[".as_slice(), DOCUMENT, b"] null"].concat(),
        [b"[".as_slice(), DOCUMENT, b" null]"].concat(),
        [b"[{},".as_slice(), DOCUMENT, b"]"].concat(),
    ] {
        assert_eq!(
            decode(&input, Some(&format()), &limits, Root::Collection).unwrap_err(),
            Error::InvalidDocument,
        );
    }
}

#[test]
fn collection_count_short_circuit_keeps_whole_input_safety_checks() {
    let limits = Limits {
        maximum_document_count: 1,
        ..Limits::default()
    };
    let over_count = [b"[".as_slice(), DOCUMENT, b",null]"].concat();
    assert_eq!(
        decode(
            &over_count,
            Some(&format()),
            &Limits {
                maximum_byte_count: over_count.len() - 1,
                ..limits.clone()
            },
            Root::Collection,
        )
        .unwrap_err(),
        Error::InputTooLarge {
            byte_count: over_count.len(),
            maximum_byte_count: over_count.len() - 1,
        },
    );
    let malformed_unicode = [b"[".as_slice(), DOCUMENT, br#", "\ud800"]"#].concat();
    assert_eq!(
        decode(
            &malformed_unicode,
            Some(&format()),
            &limits,
            Root::Collection
        )
        .unwrap_err(),
        Error::InvalidDocument,
    );
    let too_deep = [b"[".as_slice(), DOCUMENT, b",[[[null]]]]"].concat();
    assert_eq!(
        decode(
            &too_deep,
            Some(&format()),
            &Limits {
                maximum_depth: 3,
                ..limits
            },
            Root::Collection,
        )
        .unwrap_err(),
        Error::NestingTooDeep { maximum_depth: 3 },
    );
}

#[test]
fn reads_an_actual_legacy_swift_presetkit_export() {
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/documents/legacy-presetkit-2.json"
    ));
    let documents = decode(input, Some(&format()), &Limits::default(), Root::Collection).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].metadata.title.as_deref(), Some("Legacy"));
    assert_eq!(
        documents[0].payload.get(),
        r#"{"integer":9007199254740993,"name":"日本語😀"}"#,
    );
}

#[test]
fn invalid_recorded_identifiers_do_not_become_unbounded_mismatch_diagnostics() {
    for identifier in ["invalid/id".to_owned(), "a".repeat(1025)] {
        let input = std::str::from_utf8(DOCUMENT)
            .unwrap()
            .replace("com.example.presets", &identifier);
        assert!(matches!(
            decode(
                input.as_bytes(),
                Some(&format()),
                &Limits::default(),
                Root::Document
            ),
            Err(Error::InvalidDocument)
        ));
    }
}

#[test]
fn opaque_payload_strings_and_keys_require_well_formed_escaped_unicode() {
    for payload in [
        r#""\ud800""#,
        r#""\udc00""#,
        r#"{"\ud800":true}"#,
        r#""\ud800x""#,
        r#""\ud800\ud800""#,
    ] {
        let input = format!(
            r#"{{"formatVersion":{{"identifier":"com.example.presets","version":1}},"payload":{payload}}}"#
        );
        assert!(matches!(
            decode(
                input.as_bytes(),
                Some(&format()),
                &Limits::default(),
                Root::Document
            ),
            Err(Error::InvalidDocument)
        ));
    }
    for payload in [
        r#""\ud83d\ude00""#,
        r#""\n\t\\\"""#,
        r#"{"emoji":"😀","number":9007199254740993}"#,
    ] {
        let input = format!(
            r#"{{"formatVersion":{{"identifier":"com.example.presets","version":1}},"payload":{payload}}}"#
        );
        let documents = decode(
            input.as_bytes(),
            Some(&format()),
            &Limits::default(),
            Root::Document,
        )
        .unwrap();
        assert_eq!(documents[0].payload.get(), payload);
    }
}
