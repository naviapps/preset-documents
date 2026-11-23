import Foundation
import PresetDocuments
import XCTest

final class PresetFormatCodableTests: XCTestCase {
  func testPortableFormatJSONUsesCanonicalKeysAndValidatesOriginalBytes() throws {
    let canonical = Data(
      #"{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":3}"#.utf8)
    let format = try PresetFormat(json: canonical)
    XCTAssertEqual(
      format,
      try PresetFormat(
        identifier: "com.example.presets", minimumSupportedVersion: 2, currentVersion: 3))
    XCTAssertEqual(try format.encodeJSON(), canonical)
    let duplicate = Data(
      #"{"identifier":"com.example.presets","minimumSupportedVersion":2,"minimumSupportedVersion":1,"currentVersion":3}"#
        .utf8)
    for invalid in [duplicate, Data([0xff])] {
      XCTAssertThrowsError(try PresetFormat(json: invalid)) {
        XCTAssertEqual($0 as? PresetDocumentError, .invalidConfiguration)
      }
    }
  }

  func testDecodingPreservesValidRangeAndEncodedFields() throws {
    let data = Data(
      #"{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":3}"#.utf8)
    let format = try JSONDecoder().decode(PresetFormat.self, from: data)
    XCTAssertEqual(
      format,
      try PresetFormat(
        identifier: "com.example.presets", minimumSupportedVersion: 2, currentVersion: 3))

    let encoded = try JSONEncoder().encode(format)
    let fields = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
    XCTAssertEqual(Set(fields.keys), ["identifier", "minimumSupportedVersion", "currentVersion"])
    XCTAssertEqual(try JSONDecoder().decode(PresetFormat.self, from: encoded), format)
  }

  func testDecodingPreservesNativeFormatValidationErrors() {
    for input in [
      #"{"identifier":"com.example.presets","minimumSupportedVersion":5,"currentVersion":1}"#,
      #"{"identifier":"bad/id","minimumSupportedVersion":1,"currentVersion":1}"#,
    ] {
      XCTAssertThrowsError(try JSONDecoder().decode(PresetFormat.self, from: Data(input.utf8))) {
        XCTAssertEqual($0 as? PresetDocumentError, .invalidConfiguration)
      }
    }
  }

  func testDecodingStillRequiresEveryStoredField() {
    for (input, missing) in [
      (#"{"identifier":"com.example.presets","currentVersion":3}"#, "minimumSupportedVersion"),
      (#"{"identifier":"com.example.presets","minimumSupportedVersion":1}"#, "currentVersion"),
    ] {
      XCTAssertThrowsError(try JSONDecoder().decode(PresetFormat.self, from: Data(input.utf8))) {
        error in
        guard case DecodingError.keyNotFound(let key, _) = error else {
          return XCTFail("Expected a missing stored field, received \(error)")
        }
        XCTAssertEqual(key.stringValue, missing)
      }
    }
  }
}
