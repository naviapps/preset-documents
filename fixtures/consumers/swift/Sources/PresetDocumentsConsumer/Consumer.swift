import Foundation
import PresetDocuments
import PresetDocumentsFileIO

private struct Payload: Codable, Sendable, Equatable {
  let title: String
  let integer: UInt64
}

private struct AcceptanceFailure: Error {
  let message: String
}

private func require(_ condition: Bool, _ message: String) throws {
  guard condition else { throw AcceptanceFailure(message: message) }
}

@main
struct PresetDocumentsConsumer {
  static func main() async throws {
    let originalFormat = try PresetFormat(
      identifier: "com.example.standalone-presets", minimumSupportedVersion: 2, currentVersion: 3)
    let format = try JSONDecoder().decode(
      PresetFormat.self, from: JSONEncoder().encode(originalFormat))
    try require(format == originalFormat, "Codable did not preserve the accepted format range")
    let portable = try originalFormat.encodeJSON()
    let imported = try PresetFormat(json: portable)
    let expectedFormatJSON = Data(
      #"{"identifier":"com.example.standalone-presets","minimumSupportedVersion":2,"currentVersion":3}"#
        .utf8)
    try require(portable == expectedFormatJSON, "Portable JSON changed the canonical object")
    try require(imported == originalFormat, "Portable JSON did not preserve the accepted range")
    do {
      _ = try JSONDecoder().decode(
        PresetFormat.self,
        from: Data(
          #"{"identifier":"com.example.standalone-presets","minimumSupportedVersion":5,"currentVersion":1}"#
            .utf8))
      throw AcceptanceFailure(message: "Decoded format bypassed native version-range validation")
    } catch let error as PresetDocumentError {
      try require(error == .invalidConfiguration, "Format validation lost its public native error")
    }
    let document = format.makeDocument(
      payload: Payload(title: "Standalone 日本語😀", integer: 9_007_199_254_740_993),
      metadata: PresetMetadata(
        id: "external-consumer", title: "Native round trip",
        createdAt: Date(timeIntervalSinceReferenceDate: -0.125)))
    let limits = PresetDocumentLimits(maximumByteCount: 1_000_000)
    do {
      _ = try PresetJSONCodec.encode(
        [document, document, document], limits: .init(maximumDocumentCount: 1))
      throw AcceptanceFailure(message: "The installed codec accepted too many authored documents")
    } catch let error as PresetDocumentError {
      try require(
        error == .tooManyDocuments(documentCount: 3, maximumDocumentCount: 1),
        "The installed codec did not report the full typed collection count")
    }
    let encoded = try PresetJSONCodec.encode(document, limits: limits)
    let decoded = try PresetJSONCodec.decodeDocument(
      Payload.self, from: encoded, matching: format, limits: limits)
    try require(decoded == document, "The native codec did not preserve the document")
    try require(
      decoded.metadata.createdAtRFC3339 == "2000-12-31T23:59:59.875Z",
      "The shipped native library did not preserve the timestamp fraction")
    let authored = PresetMetadata(createdAtRFC3339: "2000-12-31t23:59:59.875z")
    try require(
      authored.createdAt == decoded.metadata.createdAt
        && authored.createdAtRFC3339 == "2000-12-31t23:59:59.875z",
      "The shipped native library did not project the authored timestamp")

    let minimal = format.makeDocument(payload: "<日本語>&😀")
    let minimalBytes = try PresetJSONCodec.encode(minimal)
    try require(
      String(decoding: minimalBytes, as: UTF8.self).contains(#""payload":"<日本語>&😀""#),
      "The installed codec expanded compact string escapes")
    let limited = try PresetJSONCodec.encode(
      minimal, limits: .init(maximumByteCount: minimalBytes.count))
    try require(
      limited == minimalBytes, "The installed codec did not honor the exact compact byte budget")

    let uriText = "https://example.test/%E6%97%A5?view=%2f"
    guard let uri = URL(string: uriText) else {
      throw AcceptanceFailure(message: "Could not construct the valid URI fixture")
    }
    let uriBytes = try PresetJSONCodec.encode(format.makeDocument(payload: uri))
    let decodedURI = try PresetJSONCodec.decodeDocument(URL.self, from: uriBytes, matching: format)
    try require(
      decodedURI.payload.absoluteString == uriText,
      "The installed codec changed a valid URI's spelling")

    let manager = FileManager.default
    let directory = manager.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try manager.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? manager.removeItem(at: directory) }
    let destination = directory.appendingPathComponent("日本語.preset")
    do {
      try await PresetDocumentFiles.writeNew(
        encoded, to: destination, maximumByteCount: encoded.count - 1)
      throw AcceptanceFailure(message: "The installed file adapter allowed an oversized write")
    } catch let error as PresetDocumentError {
      try require(
        error == .outputTooLarge(byteCount: encoded.count, maximumByteCount: encoded.count - 1),
        "The oversized write did not report the submitted size and budget")
    }
    try require(!manager.fileExists(atPath: destination.path), "A refused write created a file")
    try await PresetDocumentFiles.writeNew(
      encoded, to: destination, maximumByteCount: limits.maximumByteCount)
    let saved = try await PresetDocumentFiles.read(
      from: destination, maximumByteCount: limits.maximumByteCount)
    try require(saved == encoded, "The native file adapter did not preserve the encoded bytes")
    let normalized = try PresetJSONCodec.normalizeDocumentEnvelope(
      from: saved, matching: format, limits: limits)
    try require(normalized == saved, "The installed codec changed an already normalized document")

    do {
      try await PresetDocumentFiles.writeNew(Data("must not replace".utf8), to: destination)
      throw AcceptanceFailure(message: "Create-only publication replaced an occupied destination")
    } catch let error as PresetDocumentError {
      try require(error == .destinationAlreadyExists, "The collision error lost its public type")
    }
    let afterCollision = try await PresetDocumentFiles.read(from: destination)
    try require(afterCollision == encoded, "A rejected write changed the original document")

    let atomicDestination = directory.appendingPathComponent("atomic.preset")
    try await PresetDocumentFiles.writeAtomically(encoded, to: atomicDestination)
    let atomicSaved = try await PresetDocumentFiles.read(from: atomicDestination)
    try require(atomicSaved == encoded, "Atomic creation changed the encoded bytes")
    let entries = try manager.contentsOfDirectory(atPath: directory.path).sorted()
    try require(
      entries == [atomicDestination.lastPathComponent, destination.lastPathComponent].sorted(),
      "The file adapter left staged files")

    let approval: PresetReplacementRevision
    do {
      approval = try await PresetDocumentFiles.captureReplacement(at: destination)
    } catch PresetDocumentError.approvedReplacementUnsupported {
      print("Standalone Swift consumer passed. Approved replacement reported unsupported.")
      return
    }
    let replacement = try PresetJSONCodec.encode(format.makeDocument(payload: UInt64(42)))
    let outcome = try await PresetDocumentFiles.replaceApproved(approval, with: replacement)
    guard case .committed(_, .matchesApprovedFile) = outcome else {
      throw AcceptanceFailure(message: "Expected a committed exchange retaining the approved item")
    }
    let displaced = try await PresetDocumentFiles.read(from: outcome.displacedPresetURL)
    let committed = try await PresetDocumentFiles.read(from: destination)
    try require(
      displaced == encoded && committed == replacement,
      "The installed replacement adapter lost the retained or committed bytes")
    print("Standalone Swift consumer passed, including approved replacement recovery.")
  }
}
