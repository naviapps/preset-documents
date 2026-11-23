import Foundation
import PresetDocuments
import XCTest

final class PresetMetadataTimestampTests: XCTestCase {
  func testMetadataBinaryFractionRoundTripsExactlyInDocumentsAndCollections() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let date = Date(timeIntervalSince1970: 1_700_000_000.125)
    let document = format.makeDocument(payload: 42, metadata: .init(createdAt: date))
    let data = try PresetJSONCodec.encode(document)
    let encoded = try JSONDecoder().decode(TimestampDocument.self, from: data)
    XCTAssertEqual(encoded.metadata.createdAt, "2023-11-14T22:13:20.125Z")
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocument(Int.self, from: data, matching: format).metadata.createdAt,
      date)

    let noTimestamp = format.makeDocument(payload: 43)
    let collection = try PresetJSONCodec.encode([document, noTimestamp])
    let encodedCollection = try JSONDecoder().decode([TimestampDocument].self, from: collection)
    XCTAssertEqual(
      encodedCollection.map(\.metadata.createdAt), ["2023-11-14T22:13:20.125Z", nil])
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocuments(Int.self, from: collection, matching: format),
      [document, noTimestamp])
  }

  func testMetadataRetainsSubmillisecondPrecisionWithinNativeDateResolution() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let date = Date(timeIntervalSince1970: 1_700_000_000.1234567)
    let document = format.makeDocument(payload: 42, metadata: .init(createdAt: date))
    let data = try PresetJSONCodec.encode(document)
    let encoded = try JSONDecoder().decode(TimestampDocument.self, from: data)
    let timestamp = try XCTUnwrap(encoded.metadata.createdAt)
    let components = timestamp.split(separator: ".")
    XCTAssertEqual(components.count, 2)
    let fraction = try XCTUnwrap(components.last?.dropLast())
    XCTAssertGreaterThan(fraction.count, 3)
    let restored = try XCTUnwrap(
      PresetJSONCodec.decodeDocument(Int.self, from: data, matching: format).metadata.createdAt)
    XCTAssertEqual(
      restored.timeIntervalSinceReferenceDate, date.timeIntervalSinceReferenceDate,
      accuracy: date.timeIntervalSinceReferenceDate.ulp)
  }

  func testMetadataPrecisionDoesNotChangePayloadDateEncoding() throws {
    struct Payload: Encodable {
      struct Metadata: Encodable { let createdAt: Date }
      let recordedAt: Date
      let metadata: Metadata
    }
    struct WirePayload: Decodable {
      struct Metadata: Decodable { let createdAt: String }
      let recordedAt: String
      let metadata: Metadata
    }
    struct WireDocument: Decodable {
      let metadata: TimestampDocument.Metadata
      let payload: WirePayload
    }
    let format = try PresetFormat(identifier: "com.example.presets")
    let date = Date(timeIntervalSince1970: 1_700_000_000.125)
    let document = format.makeDocument(
      payload: Payload(recordedAt: date, metadata: .init(createdAt: date)),
      metadata: .init(createdAt: date))
    let wire = try JSONDecoder().decode(WireDocument.self, from: PresetJSONCodec.encode(document))
    XCTAssertEqual(wire.metadata.createdAt, "2023-11-14T22:13:20.125Z")
    XCTAssertEqual(wire.payload.recordedAt, "2023-11-14T22:13:20Z")
    XCTAssertEqual(wire.payload.metadata.createdAt, wire.payload.recordedAt)
  }

  func testTypedMetadataPreservesFoundationCalendarAndNegativeFraction() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let date = Date(timeIntervalSince1970: -12_219_379_199.875)
    let document = format.makeDocument(payload: 42, metadata: .init(createdAt: date))
    let data = try PresetJSONCodec.encode(document)
    let encoded = try JSONDecoder().decode(TimestampDocument.self, from: data)
    XCTAssertEqual(encoded.metadata.createdAt, "1582-10-04T00:00:00.125Z")
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocument(Int.self, from: data, matching: format).metadata.createdAt,
      date)
  }

  func testNonfiniteMetadataDatesFailWithoutConvertingToAnInteger() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    for interval in [Double.infinity, -.infinity, .nan] {
      let document = format.makeDocument(
        payload: 42, metadata: .init(createdAt: Date(timeIntervalSinceReferenceDate: interval)))
      XCTAssertThrowsError(try PresetJSONCodec.encode(document)) {
        guard case EncodingError.invalidValue(_, let context) = $0 else {
          return XCTFail("Expected a typed invalid metadata Date error: \($0)")
        }
        XCTAssertEqual(context.codingPath.map(\.stringValue), ["metadata", "createdAt"])
      }
    }
  }

  func testPortableTimestampTextThroughBothCodecRoots() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let input = "2026-10-30 12:34:56.123456789+09:00"
    let canonical = "2026-10-30T12:34:56.123456789+09:00"
    let authored = format.makeDocument(payload: 42, metadata: .init(createdAtRFC3339: input))
    XCTAssertEqual(authored.metadata.createdAtRFC3339, input)

    let saved = try JSONSerialization.data(withJSONObject: wireDocument(input))
    let restored = try PresetJSONCodec.decodeDocument(Int.self, from: saved, matching: format)
    XCTAssertEqual(restored.metadata.createdAtRFC3339, canonical)
    XCTAssertEqual(restored.payload, 42)
    let encoded = try PresetJSONCodec.encode(authored)
    XCTAssertEqual(
      try JSONDecoder().decode(TimestampDocument.self, from: encoded).metadata.createdAt,
      canonical)
    XCTAssertEqual(try PresetJSONCodec.encode(restored), encoded)

    let savedCollection = try JSONSerialization.data(withJSONObject: [wireDocument(input)])
    let restoredCollection = try PresetJSONCodec.decodeDocuments(
      Int.self, from: savedCollection, matching: format)
    XCTAssertEqual(restoredCollection, [restored])
    let encodedCollection = try PresetJSONCodec.encode([authored])
    XCTAssertEqual(
      try JSONDecoder().decode([TimestampDocument].self, from: encodedCollection)
        .map(\.metadata.createdAt), [canonical])
    XCTAssertEqual(try PresetJSONCodec.encode(restoredCollection), encodedCollection)
  }

  func testMetadataCodableRetainsItsDateStrategiesAndPreciseText() throws {
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .secondsSince1970
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .secondsSince1970
    let typed = PresetMetadata(createdAt: Date(timeIntervalSince1970: 1_700_000_000.125))
    let typedData = try encoder.encode(typed)
    struct NumericMetadata: Decodable { let createdAt: Double }
    XCTAssertEqual(
      try JSONDecoder().decode(NumericMetadata.self, from: typedData).createdAt, 1_700_000_000.125)
    XCTAssertEqual(try decoder.decode(PresetMetadata.self, from: typedData), typed)

    let precise = PresetMetadata(createdAtRFC3339: "2026-10-30T12:34:56.123456789+09:00")
    let preciseData = try encoder.encode(precise)
    XCTAssertNotNil(precise.createdAt)
    XCTAssertEqual(try decoder.decode(PresetMetadata.self, from: preciseData), precise)
  }

  func testTextMetadataUsesThePortableDateContract() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let canonical = "2026-10-30T12:34:56.125+09:00"
    let expected = Date(timeIntervalSince1970: 1_793_331_296.125)
    for text in ["2026-10-30t12:34:56.125z", "2026-10-30 12:34:56.125+09:00"] {
      let metadata = PresetMetadata(createdAtRFC3339: text)
      let date = text.hasSuffix("z") ? expected.addingTimeInterval(9 * 60 * 60) : expected
      XCTAssertEqual(metadata.createdAt, date)
      XCTAssertEqual(metadata.createdAtRFC3339, text)
      let encoded = try PresetJSONCodec.encode(
        format.makeDocument(payload: 42, metadata: metadata))
      let decoded = try PresetJSONCodec.decodeDocument(Int.self, from: encoded, matching: format)
      XCTAssertEqual(decoded.metadata.createdAt, date)
      if text.hasSuffix("+09:00") {
        XCTAssertEqual(decoded.metadata.createdAtRFC3339, canonical)
      }
    }
    let invalid = PresetMetadata(createdAtRFC3339: "2026-02-29T12:34:56Z")
    XCTAssertNil(invalid.createdAt)
    XCTAssertEqual(invalid.createdAtRFC3339, "2026-02-29T12:34:56Z")
  }

  func testMetadataCodableRespectsCustomStringDateStrategy() throws {
    let decoder = JSONDecoder()
    let customDate = Date(timeIntervalSince1970: 42)
    decoder.dateDecodingStrategy = .custom { _ in customDate }
    let bytes = Data(#"{"createdAt":"2026-10-30T12:34:56.123456789+09:00"}"#.utf8)
    XCTAssertEqual(
      try decoder.decode(PresetMetadata.self, from: bytes),
      PresetMetadata(createdAt: customDate))
  }

  func testInvalidTimestampThroughBothCodecRoots() throws {
    let format = try PresetFormat(identifier: "com.example.presets")
    let input = "2026-02-29T12:34:56Z"
    let metadata = PresetMetadata(createdAtRFC3339: input)
    XCTAssertNil(metadata.createdAt)
    XCTAssertEqual(metadata.createdAtRFC3339, input)
    let document = format.makeDocument(payload: 42, metadata: metadata)
    let saved = try JSONSerialization.data(withJSONObject: wireDocument(input))
    let collection = try JSONSerialization.data(withJSONObject: [wireDocument(input)])
    XCTAssertThrowsError(try PresetJSONCodec.encode(document)) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
    XCTAssertThrowsError(try PresetJSONCodec.encode([document])) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocument(Int.self, from: saved, matching: format)
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocuments(Int.self, from: collection, matching: format)
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
  }

  private func wireDocument(_ timestamp: String) -> [String: Any] {
    [
      "formatVersion": ["identifier": "com.example.presets", "version": 1],
      "metadata": ["createdAt": timestamp],
      "payload": 42,
    ]
  }

  private struct TimestampDocument: Decodable {
    struct Metadata: Decodable { let createdAt: String? }
    let metadata: Metadata
  }
}
