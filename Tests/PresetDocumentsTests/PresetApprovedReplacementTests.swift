import Darwin
import Foundation
import PresetDocuments
import PresetDocumentsFileIO
import XCTest

@MainActor
final class PresetApprovedReplacementTests: XCTestCase {
  func testApprovedReplacementRetainsTheDisplacedFile() async throws {
    let root = try makeAPFSDirectory()
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("承認済み \"export\".preset")
    let original = Data("approved original".utf8)
    let replacement = Data("replacement".utf8)
    try original.write(to: file)

    let approval = try await PresetDocumentFiles.captureReplacement(at: file)
    let outcome = try await PresetDocumentFiles.replaceApproved(
      approval, with: replacement, maximumByteCount: replacement.count)

    guard case .committed(let recovery, .matchesApprovedFile) = outcome else {
      return XCTFail("Expected a committed exchange with the approved file retained.")
    }
    XCTAssertEqual(outcome.recoveryDirectory, recovery)
    XCTAssertEqual(recovery.deletingLastPathComponent().resolvingSymlinksInPath(), root)
    XCTAssertEqual(outcome.displacedPresetURL.lastPathComponent, "displaced-preset")
    XCTAssertEqual(try Data(contentsOf: file), replacement)
    XCTAssertEqual(try Data(contentsOf: outcome.displacedPresetURL), original)
  }

  func testApprovedReplacementReportsChangedDestinationWithoutWriting() async throws {
    let root = try makeAPFSDirectory()
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("reviewed.preset")
    let former = root.appendingPathComponent("former.preset")
    let original = Data("approved original".utf8)
    let replacement = Data("must not publish".utf8)
    try original.write(to: file)
    let approval = try await PresetDocumentFiles.captureReplacement(at: file)
    try FileManager.default.moveItem(at: file, to: former)

    let competing = Data("changed destination".utf8)
    try competing.write(to: file)
    do {
      _ = try await PresetDocumentFiles.replaceApproved(approval, with: replacement)
      XCTFail("Approval must not carry over to a different file.")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .destinationChanged)
    }
    XCTAssertEqual(try Data(contentsOf: file), competing)
    XCTAssertEqual(try Data(contentsOf: former), original)
  }

  func testApprovedReplacementHonorsBudgetAndPreDispatchCancellation() async throws {
    let root = try makeAPFSDirectory()
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("approved.preset")
    let original = Data("original".utf8)
    let replacement = Data("replacement".utf8)
    try original.write(to: file)
    let cancelledCapture = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      return try await PresetDocumentFiles.captureReplacement(at: file)
    }
    do {
      _ = try await cancelledCapture.value
      XCTFail("Expected capture to honor pre-dispatch cancellation.")
    } catch is CancellationError {}

    let approval = try await PresetDocumentFiles.captureReplacement(at: file)
    for count in [replacement.count, 64 * 1024 * 1024 + 1] {
      do {
        _ = try await PresetDocumentFiles.replaceApproved(
          approval, with: Data(count: count), maximumByteCount: 1)
        XCTFail("Expected the replacement byte budget to be enforced.")
      } catch let error as PresetDocumentError {
        XCTAssertEqual(error, .outputTooLarge(byteCount: count, maximumByteCount: 1))
      }
    }
    let cancelledReplacement = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      return try await PresetDocumentFiles.replaceApproved(approval, with: replacement)
    }
    do {
      _ = try await cancelledReplacement.value
      XCTFail("Expected replacement to honor pre-dispatch cancellation.")
    } catch is CancellationError {}
    XCTAssertEqual(try Data(contentsOf: file), original)
    XCTAssertEqual(
      try FileManager.default.contentsOfDirectory(atPath: root.path), [file.lastPathComponent])
  }

  func testCaptureReplacementRejectsNonLocalAuthority() async throws {
    var url = URLComponents()
    url.scheme = "file"
    url.host = "storage.example.invalid"
    url.path = "/approved.preset"
    do {
      _ = try await PresetDocumentFiles.captureReplacement(at: XCTUnwrap(url.url))
      XCTFail("A nonlocal file URL must not capture a local destination.")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .invalidConfiguration)
    }
  }

  private func makeAPFSDirectory() throws -> URL {
    let temporary = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
    var info = statfs()
    let result = temporary.withUnsafeFileSystemRepresentation { path in
      guard let path else { return Int32(-1) }
      return statfs(path, &info)
    }
    guard result == 0 else {
      throw PresetDocumentError.fileAccess(kind: "CouldNotDetermineTestFilesystem")
    }
    let type = withUnsafeBytes(of: &info.f_fstypename) {
      String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self)
    }
    guard type == "apfs" else { throw XCTSkip("Approved replacement requires macOS APFS.") }
    let root = temporary.appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(
      at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    return root
  }
}
