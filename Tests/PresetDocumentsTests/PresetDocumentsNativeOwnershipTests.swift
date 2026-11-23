import CPresetDocuments
import Foundation
import PresetDocumentsFileIO
import XCTest

@testable import PresetDocuments

@MainActor
final class PresetDocumentsNativeOwnershipTests: XCTestCase {
  func testLargeReplySharesNativeStorageAcrossCallsRemovalCopiesSlicesAndCopyOnWrite() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("owned.preset")
    let payload = Data(repeating: 0xa5, count: 1024 * 1024)
    try payload.write(to: file)
    let options = try JSONEncoder().encode(NativeFileOptions(path: file.path))

    let retained = try autoreleasepool { () throws -> (Data, [UInt8], UInt?) in
      let owner = try nativeReply(5, configuration: options)
      var count = 0
      var status: UInt32 = 99
      let pointer = preset_documents_buffer_data(owner, &count, &status)
      // Keep only the numeric address after consume; never dereference a borrowed pointer later.
      let address = pointer.map { UInt(bitPattern: $0) }
      XCTAssertNotNil(address)
      XCTAssertEqual(status, 0)
      XCTAssertEqual(count, payload.count)
      let reply = try PresetDocumentsNative.consume(owner)
      XCTAssertEqual(
        reply.withUnsafeBytes { $0.baseAddress.map { UInt(bitPattern: $0) } }, address)

      let copied = reply
      let range = (reply.startIndex + 31)..<(reply.endIndex - 19)
      let slice = reply[range]
      let expectedSlice = Array(payload[range])
      let sliceAddress = address.map { $0 + UInt(range.lowerBound) }
      try Data([0, 255, 128]).write(to: file)
      XCTAssertEqual(
        try PresetDocumentsNative.consume(nativeReply(5, configuration: options)),
        Data([0, 255, 128]))
      try FileManager.default.removeItem(at: root)

      var changed = copied
      changed[changed.startIndex] ^= 0xff
      XCTAssertNotEqual(changed.first, reply.first)
      XCTAssertEqual(copied, payload)
      XCTAssertEqual(reply, payload)
      XCTAssertEqual(Array(slice), expectedSlice)
      XCTAssertEqual(
        slice.withUnsafeBytes { $0.baseAddress.map { UInt(bitPattern: $0) } }, sliceAddress)
      return (slice, expectedSlice, sliceAddress)
    }
    // Only the slice retains the native result after the original Data and its copies leave scope.
    XCTAssertEqual(Array(retained.0), retained.1)
    XCTAssertEqual(
      retained.0.withUnsafeBytes { $0.baseAddress.map { UInt(bitPattern: $0) } }, retained.2)
  }

  func testPublicFileDataSurvivesReplacementRemovalAndDetachedTaskHandoff() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("public.preset")
    let payload = Data(repeating: 0x5a, count: 1024 * 1024)
    try await PresetDocumentFiles.writeAtomically(payload, to: file)
    let reply = try await PresetDocumentFiles.read(from: file)
    try await PresetDocumentFiles.writeAtomically(Data("replacement".utf8), to: file)
    try FileManager.default.removeItem(at: root)

    let checks = await Task.detached {
      var changed = reply
      changed[changed.startIndex] ^= 0xff
      return (reply == payload, changed.first != reply.first)
    }.value
    XCTAssertTrue(checks.0)
    XCTAssertTrue(checks.1)
    XCTAssertEqual(reply, payload)
  }

  func testPublicCodecDataRemainsOwnedAcrossLaterCodecCalls() throws {
    let format = try PresetFormat(identifier: "com.example.owned")
    let payload = String(repeating: "a", count: 1024 * 1024)
    let documents = [format.makeDocument(payload: payload)]
    let limits = PresetDocumentLimits(maximumByteCount: 4 * 1024 * 1024)
    let encoded = try PresetJSONCodec.encode(documents, limits: limits)
    let normalized = try PresetJSONCodec.normalizeDocumentEnvelopes(
      from: encoded, matching: format, limits: limits)
    let replacement = try PresetJSONCodec.encode([format.makeDocument(payload: "replacement")])
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocuments(String.self, from: replacement, matching: format)
        .first?.payload, "replacement")
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocuments(
        String.self, from: encoded, matching: format, limits: limits), documents)
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocuments(
        String.self, from: normalized, matching: format, limits: limits), documents)
    XCTAssertEqual(normalized, encoded)
  }

  func testEmptySmallTypedFailureAndNilRepliesKeepTheirContracts() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("empty.preset")
    try Data().write(to: file)
    let options = try JSONEncoder().encode(NativeFileOptions(path: file.path))
    for operation in [UInt32(5), 6] {
      let owner = try nativeReply(operation, configuration: options)
      var count = 1
      var status: UInt32 = 99
      _ = preset_documents_buffer_data(owner, &count, &status)
      XCTAssertEqual(status, 0)
      XCTAssertEqual(count, 0)
      XCTAssertEqual(try PresetDocumentsNative.consume(owner), Data())
    }
    XCTAssertEqual(try Data(contentsOf: file), Data())
    let limits = PresetDocumentLimits(maximumByteCount: 4096)
    let small = try PresetDocumentsNative.consume(
      nativeReply(4, data: JSONEncoder().encode(limits)))
    XCTAssertEqual(try JSONDecoder().decode(PresetDocumentLimits.self, from: small), limits)
    XCTAssertThrowsError(
      try PresetDocumentsNative.consume(
        nativeReply(4, data: JSONEncoder().encode(PresetDocumentLimits(maximumByteCount: 0))))
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidConfiguration)
    }
    XCTAssertThrowsError(try PresetDocumentsNative.consume(nil)) {
      XCTAssertEqual($0 as? PresetDocumentError, .nativeFailure)
    }
    let format = try PresetFormat(identifier: "com.example.after-error")
    let encoded = try PresetJSONCodec.encode(format.makeDocument(payload: 42))
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocument(
        Int.self, from: encoded, matching: format
      ).payload, 42)
  }

  func testHostPayloadDecoderCanReenterAndThrowWithoutChangingErrorCategory() throws {
    let format = try PresetFormat(identifier: "com.example.reentry")
    let encoded = try PresetJSONCodec.encode(format.makeDocument(payload: "outer"))
    let document = try PresetJSONCodec.decodeDocument(
      ReenteringPayload.self, from: encoded, matching: format)
    XCTAssertEqual(document.payload.value, "outer")
    XCTAssertThrowsError(
      try PresetJSONCodec.decodeDocument(ReenteringFailure.self, from: encoded, matching: format)
    ) {
      XCTAssertEqual($0 as? PresetDocumentError, .invalidDocument)
    }
    XCTAssertEqual(
      try PresetJSONCodec.decodeDocument(String.self, from: encoded, matching: format).payload,
      "outer")
  }
}

private struct NativeFileOptions: Encodable {
  let path: String
  let maximumByteCount = 4 * 1024 * 1024
}

/// Returns a real SDK owner; each caller transfers it once to the production consume boundary.
private func nativeReply(
  _ operation: UInt32, data: Data = Data(), configuration: Data = Data()
) throws -> OpaquePointer {
  let owner = data.withUnsafeBytes { bytes in
    configuration.withUnsafeBytes { options in
      preset_documents_process(
        operation, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count,
        options.bindMemory(to: UInt8.self).baseAddress, options.count)
    }
  }
  return try XCTUnwrap(owner)
}

private func reenterPresetCodec() throws {
  let format = try PresetFormat(identifier: "com.example.nested")
  let encoded = try PresetJSONCodec.encode(format.makeDocument(payload: 42))
  let decoded = try PresetJSONCodec.decodeDocument(Int.self, from: encoded, matching: format)
  guard decoded.payload == 42 else { throw PresetDocumentError.invalidDocument }
}

private struct ReenteringPayload: Decodable {
  let value: String
  init(from decoder: any Decoder) throws {
    try reenterPresetCodec()
    value = try decoder.singleValueContainer().decode(String.self)
  }
}

private struct ReenteringFailure: Decodable {
  init(from _: any Decoder) throws {
    try reenterPresetCodec()
    throw PresetDocumentError.nativeFailure
  }
}
