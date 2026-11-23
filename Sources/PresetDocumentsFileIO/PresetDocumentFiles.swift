import Foundation
import PresetDocuments

/// An opaque observation of one local destination before the host asks for approval.
public struct PresetReplacementRevision: Sendable {
  fileprivate let url: URL
  fileprivate let nativeJSON: Data
}

/// What the native operation observed after attempting approved replacement.
public enum PresetReplacementOutcome: Sendable, Equatable {
  /// The identity of the item moved into recovery, not proof its revision remained unchanged.
  public enum DisplacedIdentity: String, Sendable, Equatable, Decodable {
    case matchesApprovedFile
    case different
    case unverified
  }

  /// The exchange committed, and the displaced item remains in recovery.
  case committed(recoveryDirectory: URL, displacedIdentity: DisplacedIdentity)
  /// Publication could not be verified. Keep the recovery directory for inspection.
  case commitUncertain(recoveryDirectory: URL)

  public var recoveryDirectory: URL {
    switch self {
    case .committed(let directory, _), .commitUncertain(let directory): directory
    }
  }

  /// The recovery entry where the displaced item is stored if it was recovered.
  /// With an uncertain commit this entry may not exist.
  public var displacedPresetURL: URL {
    recoveryDirectory.appendingPathComponent("displaced-preset")
  }
}

/// Optional filesystem adapter. Paths and selection policy belong to the host.
public enum PresetDocumentFiles {
  private struct PathOptions: Encodable {
    let path: String
  }

  private struct Options: Encodable, Sendable {
    let path: String
    let maximumByteCount: Int
  }

  private struct ReplacementResult: Decodable {
    enum State: String, Decodable {
      case committed
      case commitUncertain
    }

    let state: State
    let recoveryDirectory: String
    let displacedIdentity: PresetReplacementOutcome.DisplacedIdentity?
  }

  private static func localPath(_ url: URL) throws -> String {
    let path = url.path(percentEncoded: false)
    guard url.isFileURL,
      url.host.map({ $0.isEmpty || $0.lowercased() == "localhost" }) ?? true,
      url.user == nil, url.password == nil, url.port == nil,
      url.query == nil, url.fragment == nil,
      path.hasPrefix("/"), !path.hasSuffix("/"), !path.contains("\0"),
      url.path(percentEncoded: true).range(of: "%2f", options: .caseInsensitive) == nil
    else {
      throw PresetDocumentError.invalidConfiguration
    }
    return path
  }

  private static func configuration(_ url: URL, maximumByteCount: Int) throws -> Data {
    try JSONEncoder().encode(Options(path: localPath(url), maximumByteCount: maximumByteCount))
  }
  public static func read(from url: URL, maximumByteCount: Int = 64 * 1024 * 1024) async throws
    -> Data
  {
    let options = try configuration(url, maximumByteCount: maximumByteCount)
    try Task.checkCancellation()
    let worker = Task.detached(priority: .userInitiated) {
      try Task.checkCancellation()
      return try PresetDocumentsNative.process(5, data: Data(), configuration: options)
    }
    return try await withTaskCancellationHandler {
      do {
        let bytes = try await worker.value
        try Task.checkCancellation()
        return bytes
      } catch {
        try Task.checkCancellation()
        throw error
      }
    } onCancel: {
      worker.cancel()
    }
  }
  /// Cancellation before dispatch prevents writing. Once replacement begins,
  /// report its committed result even if cancellation arrives during the operation.
  /// An initially absent file is created exclusively and requires volume support.
  /// macOS overwrites preserve readable Finder tags on native xattr volumes.
  public static func writeAtomically(
    _ data: Data, to url: URL, maximumByteCount: Int = 64 * 1024 * 1024
  ) async throws {
    try await write(data, to: url, maximumByteCount: maximumByteCount, operation: 6)
  }

  /// Publishes a complete new file without replacing an existing file, directory or link.
  /// Throws `destinationAlreadyExists` for an occupied destination, or
  /// `atomicNewUnsupported` when the platform or volume lacks the required operation.
  /// Cancellation before dispatch prevents writing; a dispatched write reports its actual result.
  public static func writeNew(
    _ data: Data, to url: URL, maximumByteCount: Int = 64 * 1024 * 1024
  ) async throws {
    try await write(data, to: url, maximumByteCount: maximumByteCount, operation: 7)
  }

  /// Captures the revision of an existing regular file on macOS APFS before asking for approval.
  /// Use the result only for that approval. A cancellation discards any late observation.
  public static func captureReplacement(at url: URL) async throws -> PresetReplacementRevision {
    let options = try JSONEncoder().encode(PathOptions(path: localPath(url)))
    try Task.checkCancellation()
    let worker = Task.detached(priority: .userInitiated) {
      try Task.checkCancellation()
      let token = try PresetDocumentsNative.process(8, data: Data(), configuration: options)
      guard let object = try? JSONSerialization.jsonObject(with: token),
        object is [String: Any]
      else {
        throw PresetDocumentError.nativeFailure
      }
      return PresetReplacementRevision(url: url, nativeJSON: token)
    }
    return try await withTaskCancellationHandler {
      do {
        let revision = try await worker.value
        try Task.checkCancellation()
        return revision
      } catch {
        try Task.checkCancellation()
        throw error
      }
    } onCancel: {
      worker.cancel()
    }
  }

  /// Rechecks an approved destination before exchanging its name with a staged file.
  /// An absent destination is never created. macOS APFS only.
  /// This is not an atomic comparison of revisions. Keep the returned recovery directory
  /// for inspection, particularly if the displaced identity differs or publication is uncertain.
  /// Once dispatched, the native result is reported even if the task is cancelled.
  public static func replaceApproved(
    _ approval: PresetReplacementRevision, with data: Data,
    maximumByteCount: Int = 64 * 1024 * 1024
  ) async throws -> PresetReplacementOutcome {
    let options = try replacementConfiguration(approval, maximumByteCount: maximumByteCount)
    try Task.checkCancellation()
    let worker = Task.detached(priority: .userInitiated) {
      try Task.checkCancellation()
      let result = try PresetDocumentsNative.process(9, data: data, configuration: options)
      return try replacementOutcome(from: result)
    }
    return try await withTaskCancellationHandler {
      try await worker.value
    } onCancel: {
      worker.cancel()
    }
  }

  private static func replacementConfiguration(
    _ approval: PresetReplacementRevision, maximumByteCount: Int
  ) throws -> Data {
    var options = try configuration(approval.url, maximumByteCount: maximumByteCount)
    // Keep native numeric lexemes exact; Foundation is only used to validate the token as an object.
    guard options.last == 0x7D else { throw PresetDocumentError.nativeFailure }
    options.removeLast()
    options.append(contentsOf: ",\"revision\":".utf8)
    options.append(approval.nativeJSON)
    options.append(0x7D)
    return options
  }

  private static func replacementOutcome(from data: Data) throws -> PresetReplacementOutcome {
    guard let result = try? JSONDecoder().decode(ReplacementResult.self, from: data),
      result.recoveryDirectory.hasPrefix("/"), !result.recoveryDirectory.contains("\0")
    else { throw PresetDocumentError.nativeFailure }
    let directory = URL(fileURLWithPath: result.recoveryDirectory, isDirectory: true)
    switch result.state {
    case .committed:
      guard let displacedIdentity = result.displacedIdentity else {
        // The recovery path is still useful even if the rest of the receipt is incomplete.
        return .commitUncertain(recoveryDirectory: directory)
      }
      return .committed(recoveryDirectory: directory, displacedIdentity: displacedIdentity)
    case .commitUncertain:
      return .commitUncertain(recoveryDirectory: directory)
    }
  }

  private static func write(
    _ data: Data, to url: URL, maximumByteCount: Int, operation: UInt32
  ) async throws {
    let options = try configuration(url, maximumByteCount: maximumByteCount)
    try Task.checkCancellation()
    let worker = Task.detached(priority: .userInitiated) {
      try Task.checkCancellation()
      _ = try PresetDocumentsNative.process(operation, data: data, configuration: options)
    }
    try await withTaskCancellationHandler {
      try await worker.value
    } onCancel: {
      worker.cancel()
    }
  }
}
