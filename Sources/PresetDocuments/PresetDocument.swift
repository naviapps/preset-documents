import Foundation

/// Host-selected format identity and accepted document versions, validated by Rust.
public struct PresetFormat: Codable, Sendable, Equatable, Hashable {
  public let identifier: String
  public let minimumSupportedVersion: Int
  public let currentVersion: Int

  private enum CodingKeys: String, CodingKey {
    case identifier, minimumSupportedVersion, currentVersion
  }

  private struct Fields: Codable {
    let identifier: String
    let minimumSupportedVersion: Int
    let currentVersion: Int
  }

  public init(identifier: String, minimumSupportedVersion: Int = 1, currentVersion: Int = 1) throws
  {
    let fields = Fields(
      identifier: identifier, minimumSupportedVersion: minimumSupportedVersion,
      currentVersion: currentVersion)
    _ = try PresetDocumentsNative.process(1, data: JSONEncoder().encode(fields))
    self.init(validated: fields)
  }

  private init(validated fields: Fields) {
    identifier = fields.identifier
    minimumSupportedVersion = fields.minimumSupportedVersion
    currentVersion = fields.currentVersion
  }

  /// Reads the portable UTF-8 format JSON object and validates its complete version range.
  public init(json data: Data) throws {
    let canonical = try PresetDocumentsNative.process(12, data: data)
    guard let fields = try? JSONDecoder().decode(Fields.self, from: canonical) else {
      throw PresetDocumentError.nativeFailure
    }
    self.init(validated: fields)
  }

  /// Writes the portable UTF-8 format JSON object for exchange with other bindings.
  public func encodeJSON() throws -> Data {
    try PresetDocumentsNative.process(12, data: JSONEncoder().encode(self))
  }

  public init(from decoder: any Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    try self.init(
      identifier: values.decode(String.self, forKey: .identifier),
      minimumSupportedVersion: values.decode(Int.self, forKey: .minimumSupportedVersion),
      currentVersion: values.decode(Int.self, forKey: .currentVersion))
  }

  public func makeDocument<Payload>(payload: Payload, metadata: PresetMetadata = PresetMetadata())
    -> PresetDocument<Payload>
  {
    PresetDocument(
      formatVersion: PresetFormatVersion(identifier: identifier, version: currentVersion),
      metadata: metadata, payload: payload)
  }
}

/// Byte limits apply to the original input and normalized output independently.
/// Rust validates positive limits, a maximum 64 MiB budget and depth at most 128.
public struct PresetDocumentLimits: Codable, Sendable, Equatable, Hashable {
  public let maximumByteCount: Int
  /// Typed writes check the full count. Collection reads stop at the first excess element.
  public let maximumDocumentCount: Int
  public let maximumDepth: Int
  public init(
    maximumByteCount: Int = 16 * 1024 * 1024, maximumDocumentCount: Int = 65_536,
    maximumDepth: Int = 128
  ) {
    self.maximumByteCount = maximumByteCount
    self.maximumDocumentCount = maximumDocumentCount
    self.maximumDepth = maximumDepth
  }
}

public struct PresetFormatVersion: Codable, Sendable, Equatable, Hashable {
  public let identifier: String
  public let version: Int
  public init(identifier: String, version: Int) {
    self.identifier = identifier
    self.version = version
  }
}

/// Optional descriptive metadata; IDs do not imply storage or synchronization semantics.
public struct PresetMetadata: Codable, Sendable, Equatable, Hashable {
  public let id: String?
  public let title: String?
  /// Foundation's projection of the creation time, at native Date precision.
  public let createdAt: Date?
  public let producerVersion: String?
  private let retainedTimestamp: String?

  /// The creation time as RFC3339 text, including precision and offsets from the document.
  public var createdAtRFC3339: String? {
    retainedTimestamp ?? createdAt.flatMap(Self.formatTimestamp)
  }

  public init(
    id: String? = nil, title: String? = nil, createdAt: Date? = nil, producerVersion: String? = nil
  ) {
    self.id = id
    self.title = title
    self.createdAt = createdAt
    self.producerVersion = producerVersion
    retainedTimestamp = nil
  }

  /// Creates metadata with timestamp text that the Rust codec validates when saving.
  public init(
    id: String? = nil, title: String? = nil, createdAtRFC3339: String,
    producerVersion: String? = nil
  ) {
    self.init(
      id: id, title: title, date: Self.projectDate(from: createdAtRFC3339),
      timestamp: createdAtRFC3339, producerVersion: producerVersion)
  }

  private init(
    id: String?, title: String?, date: Date?, timestamp: String?, producerVersion: String?
  ) {
    self.id = id
    self.title = title
    self.createdAt = date
    self.producerVersion = producerVersion
    retainedTimestamp = timestamp == date.flatMap(Self.formatTimestamp) ? nil : timestamp
  }

  private enum CodingKeys: String, CodingKey {
    case id, title, createdAt, producerVersion
  }

  public init(from decoder: any Decoder) throws {
    let fields = try decoder.container(keyedBy: CodingKeys.self)
    let id = try fields.decodeIfPresent(String.self, forKey: .id)
    let title = try fields.decodeIfPresent(String.self, forKey: .title)
    let producerVersion = try fields.decodeIfPresent(String.self, forKey: .producerVersion)
    if let text = try? fields.decode(String.self, forKey: .createdAt) {
      let projected = Self.projectDate(from: text)
      // Honor a host's custom date strategy when it interprets the text differently.
      if let decoded = try? fields.decode(Date.self, forKey: .createdAt), decoded != projected {
        self.init(id: id, title: title, createdAt: decoded, producerVersion: producerVersion)
      } else {
        self.init(
          id: id, title: title, date: projected, timestamp: text, producerVersion: producerVersion)
      }
    } else {
      try self.init(
        id: id, title: title, createdAt: fields.decodeIfPresent(Date.self, forKey: .createdAt),
        producerVersion: producerVersion)
    }
  }

  public func encode(to encoder: any Encoder) throws {
    var fields = encoder.container(keyedBy: CodingKeys.self)
    try fields.encodeIfPresent(id, forKey: .id)
    try fields.encodeIfPresent(title, forKey: .title)
    try fields.encodeIfPresent(producerVersion, forKey: .producerVersion)
    if let retainedTimestamp {
      try fields.encode(retainedTimestamp, forKey: .createdAt)
    } else {
      try fields.encodeIfPresent(createdAt, forKey: .createdAt)
    }
  }

  private static func projectDate(from text: String) -> Date? {
    guard let canonical = PresetDocumentsNative.canonicalTimestamp(text) else { return nil }
    // Use Foundation's calendar mapping for historical Date round trips.
    return foundationDate(from: canonical)
  }

  static func foundationDate(from text: String) -> Date? {
    (try? Date.ISO8601FormatStyle(includingFractionalSeconds: true).parse(text))
      ?? (try? Date.ISO8601FormatStyle().parse(text))
  }

  private static func formatTimestamp(_ date: Date) -> String? {
    guard let parts = PresetDocumentsNative.timestampParts(date.timeIntervalSinceReferenceDate)
    else { return nil }
    let timestamp = Date(timeIntervalSinceReferenceDate: parts.whole).ISO8601Format()
    guard !parts.fraction.isEmpty else { return timestamp }
    return String(timestamp.dropLast()) + "." + parts.fraction + "Z"
  }
}

/// The host defines the payload schema and its migrations and behavior.
public struct PresetDocument<Payload> {
  public let formatVersion: PresetFormatVersion
  public let metadata: PresetMetadata
  public let payload: Payload
  public init(
    formatVersion: PresetFormatVersion, metadata: PresetMetadata = PresetMetadata(),
    payload: Payload
  ) {
    self.formatVersion = formatVersion
    self.metadata = metadata
    self.payload = payload
  }
}
extension PresetDocument: Encodable where Payload: Encodable {}
extension PresetDocument: Decodable where Payload: Decodable {}
extension PresetDocument: Sendable where Payload: Sendable {}
extension PresetDocument: Equatable where Payload: Equatable {}
extension PresetDocument: Hashable where Payload: Hashable {}

/// Shared format, limit and file failures. Payload decoding errors are invalid documents;
/// native failures remain distinct from malformed user files.
public enum PresetDocumentError: Error, Sendable, Equatable {
  case invalidConfiguration
  case invalidDocument
  case formatMismatch(expected: String, actual: String)
  case unsupportedFormatVersion(
    version: Int, minimumSupportedVersion: Int, maximumSupportedVersion: Int)
  case inputTooLarge(byteCount: Int, maximumByteCount: Int)
  case outputTooLarge(byteCount: Int, maximumByteCount: Int)
  /// Typed writes report the full count. Collection parsing reports the first excess (limit + 1).
  case tooManyDocuments(documentCount: Int, maximumDocumentCount: Int)
  case nestingTooDeep(maximumDepth: Int)
  case fileAccess(kind: String)
  case nativeFailure
  /// Create-only publication found an existing destination.
  case destinationAlreadyExists
  /// The platform or filesystem cannot publish a new file atomically without replacement.
  case atomicNewUnsupported
  /// The requested regular file was missing, unsuitable, or no longer matched before publication.
  case destinationChanged
  /// The platform or destination filesystem does not support approved replacement.
  case approvedReplacementUnsupported
}

extension PresetDocumentError: Decodable {
  private enum CodingKeys: String, CodingKey {
    case kind, code, expected, actual, version, minimumSupportedVersion, maximumSupportedVersion
    case byteCount, maximumByteCount, documentCount, maximumDocumentCount, maximumDepth
  }
  public init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    switch try c.decode(String.self, forKey: .code) {
    case "fileAccess": self = .fileAccess(kind: try c.decode(String.self, forKey: .kind))
    case "destinationAlreadyExists": self = .destinationAlreadyExists
    case "atomicNewUnsupported": self = .atomicNewUnsupported
    case "destinationChanged": self = .destinationChanged
    case "approvedReplacementUnsupported": self = .approvedReplacementUnsupported
    case "invalidConfiguration": self = .invalidConfiguration
    case "invalidDocument": self = .invalidDocument
    case "formatMismatch":
      self = .formatMismatch(
        expected: try c.decode(String.self, forKey: .expected),
        actual: try c.decode(String.self, forKey: .actual))
    case "unsupportedFormatVersion":
      self = .unsupportedFormatVersion(
        version: try c.decode(Int.self, forKey: .version),
        minimumSupportedVersion: try c.decode(Int.self, forKey: .minimumSupportedVersion),
        maximumSupportedVersion: try c.decode(Int.self, forKey: .maximumSupportedVersion))
    case "inputTooLarge":
      self = .inputTooLarge(
        byteCount: try c.decode(Int.self, forKey: .byteCount),
        maximumByteCount: try c.decode(Int.self, forKey: .maximumByteCount))
    case "outputTooLarge":
      self = .outputTooLarge(
        byteCount: try c.decode(Int.self, forKey: .byteCount),
        maximumByteCount: try c.decode(Int.self, forKey: .maximumByteCount))
    case "tooManyDocuments":
      self = .tooManyDocuments(
        documentCount: try c.decode(Int.self, forKey: .documentCount),
        maximumDocumentCount: try c.decode(Int.self, forKey: .maximumDocumentCount))
    case "nestingTooDeep":
      self = .nestingTooDeep(maximumDepth: try c.decode(Int.self, forKey: .maximumDepth))
    default: self = .nativeFailure
    }
  }
}
