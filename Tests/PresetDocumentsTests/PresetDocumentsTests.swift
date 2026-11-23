import Foundation
import PresetDocuments
import XCTest

final class PresetDocumentsTests: XCTestCase {
  func testEnvelopeNormalizationRoutesSingleAndCollectionInputs() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let payload = #"["日本語😀", "\u0061\/"]"#
    let input =
      #"{"payload":\#(payload),"formatVersion":{"version":1,"identifier":"com.example.presets"}}"#
    let document = Data(input.utf8)
    let collection = Data("[\(input)]".utf8)
    let expected =
      #"{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{},"payload":\#(payload)}"#
    let normalizedDocument = try PresetJSONCodec.normalizeDocumentEnvelope(
      from: document, matching: format)
    let normalized = try PresetJSONCodec.normalizeDocumentEnvelopes(
      from: collection, matching: format)
    XCTAssertEqual(normalizedDocument, Data(expected.utf8))
    XCTAssertEqual(normalized, Data("[\(expected)]".utf8))
    XCTAssertThrowsError(
      try PresetJSONCodec.normalizeDocumentEnvelope(
        from: document, matching: format, limits: .init(maximumByteCount: document.count))
    ) {
      XCTAssertEqual(
        $0 as? PresetDocumentError,
        .outputTooLarge(byteCount: normalizedDocument.count, maximumByteCount: document.count))
    }
    XCTAssertThrowsError(
      try PresetJSONCodec.normalizeDocumentEnvelope(from: collection, matching: format)
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
    XCTAssertThrowsError(
      try PresetJSONCodec.normalizeDocumentEnvelopes(from: document, matching: format)
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
  }

  func testInvalidWriteLimitsAndFullCollectionCountRejectBeforePayloadEncoding() throws {
    final class EncodingProbe: Encodable {
      var wasEncoded = false
      func encode(to _: any Encoder) throws {
        wasEncoded = true
        throw PresetDocumentError.nativeFailure
      }
    }
    let format = try PresetFormat(identifier: "com.example.presets")
    let payload = EncodingProbe()
    let document = format.makeDocument(payload: payload)
    let invalidLimits = PresetDocumentLimits(maximumDepth: -1)
    XCTAssertThrowsError(try PresetJSONCodec.encode(document, limits: invalidLimits)) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidConfiguration)
    }
    XCTAssertThrowsError(try PresetJSONCodec.encode([document], limits: invalidLimits)) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidConfiguration)
    }
    XCTAssertFalse(payload.wasEncoded)
    XCTAssertThrowsError(
      try PresetJSONCodec.encode(
        [document, document, document], limits: .init(maximumDocumentCount: 1))
    ) {
      XCTAssertEqual(
        $0 as? PresetDocumentError, .tooManyDocuments(documentCount: 3, maximumDocumentCount: 1))
    }
    XCTAssertFalse(payload.wasEncoded)
    XCTAssertThrowsError(try PresetJSONCodec.encode(document)) {
      XCTAssertEqual($0 as? PresetDocumentError, .nativeFailure)
    }
    XCTAssertTrue(payload.wasEncoded)
  }

  func testNativeInvalidEnvelopeBecomesInvalidDocument() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let input = Data(#"{"formatVersion":["com.example.presets",1],"payload":42}"#.utf8)
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocument(Int.self, from: input, matching: format)
    ) { XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument) }
  }

  func testObjectOnlyEnvelopesPreserveArrayPayloads() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let input = Data(
      #"[{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{"title":"日本語😀"},"payload":[{"name":"日本語😀","integer":9007199254740993}]}]"#
        .utf8)
    let documents = try PresetJSONCodec.decodeDocuments(
      [Payload].self, from: input, matching: format, limits: .init(maximumDocumentCount: 1))
    XCTAssertEqual(documents.count, 1)
    XCTAssertEqual(documents[0].metadata.title, "日本語😀")
    XCTAssertEqual(documents[0].payload, [Payload(name: "日本語😀", integer: 9_007_199_254_740_993)])
    let encoded = try PresetJSONCodec.encode(documents)
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocuments([Payload].self, from: encoded, matching: format),
      documents)
  }

  func testCollectionLimitProjectsTheNativeCountAndMaximum() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let document = format.makeDocument(payload: 42)
    let collection = try PresetJSONCodec.encode([document, document])
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocuments(
        Int.self, from: collection, matching: format, limits: .init(maximumDocumentCount: 1))
    ) {
      XCTAssertEqual(
        $0 as? PresetDocumentError,
        .tooManyDocuments(documentCount: 2, maximumDocumentCount: 1))
    }
  }
  func testMalformedEscapedUnicodeIsRejectedBeforePayloadDecoding() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let data = Data(
      #"{"formatVersion":{"identifier":"com.example.presets","version":1},"payload":"\ud800"}"#.utf8
    )
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocument(String.self, from: data, matching: format)
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
  }
  func testReadsAnActualLegacyPresetKitExport() throws {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent()
      .appendingPathComponent("fixtures/documents/legacy-presetkit-2.json")
    let format = try PresetFormat(identifier: "com.example.presets")
    let documents = try PresetJSONCodec.decodeDocuments(
      Payload.self, from: Data(contentsOf: url), matching: format)
    XCTAssertEqual(documents.count, 1)
    XCTAssertEqual(documents.first?.payload, Payload(name: "日本語😀", integer: 9_007_199_254_740_993))
    XCTAssertEqual(documents.first?.metadata.title, "Legacy")
  }
  struct Payload: Codable, Equatable {
    let name: String
    let integer: UInt64
  }
  func testRoundTripAndSavedUTF16() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let original = format.makeDocument(
      payload: Payload(name: "日本語😀", integer: 9_007_199_254_740_993),
      metadata: .init(title: "Example", createdAt: Date(timeIntervalSince1970: 1_700_000_000)))
    let data = try PresetJSONCodec.encode([original])
    for encoding in [String.Encoding.utf8, .utf16BigEndian] {
      let input = try XCTUnwrap(String(data: data, encoding: .utf8)?.data(using: encoding))
      let documents = try PresetJSONCodec.decodeDocuments(
        Payload.self, from: input, matching: format)
      XCTAssertEqual(documents, [original])
    }
  }
  func testTypedStringEscapesUseTheSharedByteBudget() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let property = "key/😀<"
    let text = "<雨>&'+`/😀\u{00a0}\u{2028}\u{e000}\"\\\n\u{001f}"
    let quoted = "\"<雨>&'+`/😀\u{00a0}\u{2028}\u{e000}\\\"\\\\\\n\\u001f\""
    let document = format.makeDocument(payload: [property: text], metadata: .init(title: text))
    let expectedDocument =
      #"{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{"title":\#(quoted)},"payload":{"key/😀<":\#(quoted)}}"#

    for collection in [false, true] {
      let expected = Data((collection ? "[" + expectedDocument + "]" : expectedDocument).utf8)
      func encode(maximumByteCount: Int) throws -> Data {
        let limits = PresetDocumentLimits(maximumByteCount: maximumByteCount)
        return try collection
          ? PresetJSONCodec.encode([document], limits: limits)
          : PresetJSONCodec.encode(document, limits: limits)
      }
      let encoded = try encode(maximumByteCount: expected.count)
      XCTAssertEqual(encoded, expected)
      let restored =
        try collection
        ? XCTUnwrap(
          PresetJSONCodec.decodeDocuments([String: String].self, from: encoded, matching: format)
            .first)
        : PresetJSONCodec.decodeDocument([String: String].self, from: encoded, matching: format)
      XCTAssertEqual(restored.payload, document.payload)
      XCTAssertEqual(restored.metadata.title, text)
      XCTAssertThrowsError(try encode(maximumByteCount: expected.count - 1)) {
        XCTAssertEqual(
          $0 as? PresetDocumentError,
          .inputTooLarge(byteCount: expected.count, maximumByteCount: expected.count - 1))
      }
    }
  }

  func testUnsetEnvelopeMetadataFitsTheCompactByteBudget() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let document = format.makeDocument(payload: "日本語")
    let expected = Data(
      #"[{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{},"payload":"日本語"}]"#
        .utf8)
    XCTAssertEqual(
      try PresetJSONCodec.encode([document], limits: .init(maximumByteCount: expected.count)),
      expected)
    XCTAssertThrowsError(
      try PresetJSONCodec.encode([document], limits: .init(maximumByteCount: expected.count - 1))
    ) {
      XCTAssertEqual(
        $0 as? PresetDocumentError,
        .inputTooLarge(byteCount: expected.count, maximumByteCount: expected.count - 1))
    }
  }
  func testSharedFormatLimitsAndNativeFailureCategories() throws {
    XCTAssertThrowsError(try PresetFormat(identifier: "bad/id")) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidConfiguration)
    }
    let format = try PresetFormat(identifier: "com.example.presets")
    let document = format.makeDocument(payload: Payload(name: "Example", integer: 5))
    let data = try PresetJSONCodec.encode(document)
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocument(
        Payload.self, from: data, matching: format, limits: .init(maximumByteCount: data.count - 1))
    ) {
      XCTAssertEqual(
        $0 as? PresetDocumentError,
        .inputTooLarge(byteCount: data.count, maximumByteCount: data.count - 1))
    }
    let other = try PresetFormat(identifier: "com.other.presets")
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocument(Payload.self, from: data, matching: other)
    ) {
      XCTAssertEqual(
        $0 as? PresetDocumentError,
        .formatMismatch(expected: other.identifier, actual: format.identifier))
    }
  }
  func testFractionalTimestampAndPayloadFailureRemainSeparate() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let data = Data(
      #"{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{"createdAt":"2026-10-30T12:34:56.1234567+09:00"},"payload":{"name":"Example","integer":1}}"#
        .utf8)
    let document = try PresetJSONCodec.decodeDocument(Payload.self, from: data, matching: format)
    XCTAssertNotNil(document.metadata.createdAt)
    struct Misleading: Decodable {
      init(from _: any Decoder) throws { throw PresetDocumentError.nativeFailure }
    }
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocument(Misleading.self, from: data, matching: format)
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
  }
}
