import CPresetDocuments
import Foundation

package enum PresetDocumentsNative {
  static func canonicalTimestamp(_ text: String) -> String? {
    struct Projection: Decodable { let canonical: String }
    guard let bytes = try? process(14, data: Data(text.utf8)) else { return nil }
    return (try? JSONDecoder().decode(Projection.self, from: bytes))?.canonical
  }

  static func timestampParts(_ seconds: Double) -> (whole: Double, fraction: String)? {
    let parts = preset_documents_timestamp_parts(seconds)
    guard parts.whole_seconds.isFinite else { return nil }
    let fraction = withUnsafeBytes(of: parts.fraction) {
      String(decoding: $0.prefix(Int(parts.fraction_length)), as: UTF8.self)
    }
    return (parts.whole_seconds, fraction)
  }

  struct Options: Encodable {
    let format: PresetFormat?
    let limits: PresetDocumentLimits
    let root: String
  }
  static func process(_ operation: UInt32, data: Data, options: Options? = nil) throws -> Data {
    let config = try options.map { try JSONEncoder().encode($0) } ?? Data()
    return try process(operation, data: data, configuration: config)
  }
  package static func process(_ operation: UInt32, data: Data, configuration optionsData: Data)
    throws -> Data
  {
    try requireMatchingABI()
    let owner = data.withUnsafeBytes { bytes in
      optionsData.withUnsafeBytes { config in
        preset_documents_process(
          operation, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count,
          config.bindMemory(to: UInt8.self).baseAddress, config.count)
      }
    }
    return try consume(owner)
  }

  package static func replaceApproved(_ data: Data, revision: Data, maximumByteCount: Int)
    throws -> Data
  {
    try requireMatchingABI()
    let owner = data.withUnsafeBytes { bytes in
      revision.withUnsafeBytes { token in
        preset_documents_replace_approved(
          bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count,
          token.bindMemory(to: UInt8.self).baseAddress, token.count, maximumByteCount)
      }
    }
    return try consume(owner)
  }

  private static func requireMatchingABI() throws {
    guard preset_documents_abi_version() == PRESET_DOCUMENTS_ABI_VERSION else {
      throw PresetDocumentError.nativeFailure
    }
  }

  static func consume(_ owner: OpaquePointer?) throws -> Data {
    guard let owner else { throw PresetDocumentError.nativeFailure }
    var count = 0
    var status: UInt32 = 0
    guard let pointer = preset_documents_buffer_data(owner, &count, &status),
      count >= 0, count <= 64 * 1024 * 1024, status == 0 || status == 1
    else {
      preset_documents_buffer_free(owner)
      throw PresetDocumentError.nativeFailure
    }
    let result: Data
    if count == 0 {
      preset_documents_buffer_free(owner)
      result = Data()
    } else {
      // Data takes ownership and releases the buffer when its storage no longer needs it.
      result = Data(
        bytesNoCopy: UnsafeMutableRawPointer(mutating: pointer), count: count,
        deallocator: .custom { _, _ in preset_documents_buffer_free(owner) })
    }
    if status == 1 { throw try JSONDecoder().decode(PresetDocumentError.self, from: result) }
    return result
  }
}

/// UTF-8 JSON output; saved UTF-8/16/32 JSON input is validated by the same Rust
/// codec on all platforms. No file/clipboard access or payload migration occurs.
/// Envelopes, formatVersion and non-null metadata must be objects; payload arrays are valid.
public enum PresetJSONCodec {
  /// Validates a single document and normalizes its envelope to UTF-8 JSON.
  /// The payload retains its exact number spelling, escapes and internal whitespace.
  public static func normalizeDocumentEnvelope(
    from data: Data, matching format: PresetFormat,
    limits: PresetDocumentLimits = .init()
  ) throws -> Data {
    try normalizeEnvelopes(from: data, matching: format, root: "document", limits: limits)
  }

  /// Validates a document collection and normalizes only its envelopes to UTF-8 JSON.
  /// Payload JSON retains its exact lexemes, including numbers, escapes and internal whitespace.
  /// Hosts own payload interpretation and migration; this is not whole-document canonicalization.
  public static func normalizeDocumentEnvelopes(
    from data: Data, matching format: PresetFormat,
    limits: PresetDocumentLimits = .init()
  ) throws -> Data {
    try normalizeEnvelopes(from: data, matching: format, root: "collection", limits: limits)
  }

  public static func encode<Payload: Encodable>(
    _ document: PresetDocument<Payload>, limits: PresetDocumentLimits = .init()
  ) throws -> Data {
    try encodeValue(EncodedDocument(document), root: "document", limits: limits)
  }
  public static func encode<Payload: Encodable>(
    _ documents: [PresetDocument<Payload>], limits: PresetDocumentLimits = .init()
  ) throws -> Data {
    try encodeValue(
      EncodedDocuments(documents: documents), root: "collection", limits: limits,
      documentCount: documents.count)
  }
  public static func decodeDocument<Payload: Decodable>(
    _: Payload.Type, from data: Data, matching format: PresetFormat,
    limits: PresetDocumentLimits = .init()
  ) throws -> PresetDocument<Payload> {
    try decodeValue(from: data, matching: format, root: "document", limits: limits)
  }
  public static func decodeDocuments<Payload: Decodable>(
    _: Payload.Type, from data: Data, matching format: PresetFormat,
    limits: PresetDocumentLimits = .init()
  ) throws -> [PresetDocument<Payload>] {
    try decodeValue(from: data, matching: format, root: "collection", limits: limits)
  }
  private static func encodeValue<Value: Encodable>(
    _ value: Value, root: String, limits: PresetDocumentLimits, documentCount: Int? = nil
  ) throws -> Data {
    let encoder = JSONEncoder()
    if let documentCount {
      let collection = TypedCollection(limits: limits, documentCount: documentCount)
      _ = try PresetDocumentsNative.process(13, data: encoder.encode(collection))
    } else {
      _ = try PresetDocumentsNative.process(4, data: encoder.encode(limits))
    }
    encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    encoder.dateEncodingStrategy = .iso8601
    let input = try encoder.encode(value)
    return try PresetDocumentsNative.process(
      11, data: input, options: .init(format: nil, limits: limits, root: root))
  }
  private static func decodeValue<Value: Decodable>(
    from data: Data, matching format: PresetFormat, root: String, limits: PresetDocumentLimits
  ) throws -> Value {
    let bytes = try normalizeEnvelopes(from: data, matching: format, root: root, limits: limits)
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .custom { decoder in
      let text = try decoder.singleValueContainer().decode(String.self)
      if let date = PresetMetadata.foundationDate(from: text) {
        return date
      }
      return try Date.ISO8601FormatStyle().parse(text)
    }
    do { return try decoder.decode(Value.self, from: bytes) } catch {
      throw PresetDocumentError.invalidDocument
    }
  }

  private static func normalizeEnvelopes(
    from data: Data, matching format: PresetFormat, root: String, limits: PresetDocumentLimits
  ) throws -> Data {
    try PresetDocumentsNative.process(
      2, data: data, options: .init(format: format, limits: limits, root: root))
  }

  private struct EncodedDocuments<Payload: Encodable>: Encodable {
    let documents: [PresetDocument<Payload>]

    func encode(to encoder: any Encoder) throws {
      var container = encoder.unkeyedContainer()
      for document in documents { try container.encode(EncodedDocument(document)) }
    }
  }

  private struct TypedCollection: Encodable {
    let limits: PresetDocumentLimits
    let documentCount: Int
  }

  private struct EncodedDocument<Payload: Encodable>: Encodable {
    let formatVersion: PresetFormatVersion
    let metadata: EncodedMetadata
    let payload: Payload

    init(_ document: PresetDocument<Payload>) {
      formatVersion = document.formatVersion
      metadata = EncodedMetadata(document.metadata)
      payload = document.payload
    }
  }

  private struct EncodedMetadata: Encodable {
    let id: String?
    let title: String?
    let createdAt: MetadataTimestamp?
    let producerVersion: String?

    init(_ metadata: PresetMetadata) {
      id = metadata.id
      title = metadata.title
      createdAt = MetadataTimestamp(metadata)
      producerVersion = metadata.producerVersion
    }
  }

  private enum MetadataTimestamp: Encodable {
    case text(String)
    case nonfinite(Date)

    init?(_ metadata: PresetMetadata) {
      if let text = metadata.createdAtRFC3339 {
        self = .text(text)
      } else if let date = metadata.createdAt {
        self = .nonfinite(date)
      } else {
        return nil
      }
    }

    func encode(to encoder: any Encoder) throws {
      switch self {
      case .text(let timestamp):
        var container = encoder.singleValueContainer()
        try container.encode(timestamp)
      case .nonfinite(let date):
        throw EncodingError.invalidValue(
          date,
          .init(
            codingPath: encoder.codingPath,
            debugDescription: "Preset metadata timestamps must be finite"))
      }
    }
  }
}
