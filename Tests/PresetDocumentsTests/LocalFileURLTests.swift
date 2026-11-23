import Foundation
import PresetDocuments
import PresetDocumentsFileIO
import XCTest

@MainActor
final class LocalFileURLTests: XCTestCase {
  func testNonLocalAuthorityCannotReadOrReplaceTheSameLocalFile() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let local = root.appendingPathComponent("reviewed.preset")
    let original = Data("reviewed".utf8)
    try original.write(to: local)
    let remote = try withHost("storage.example.invalid", local)

    do {
      _ = try await PresetDocumentFiles.read(from: remote)
      XCTFail("A remote URL must not read a local file with the same path.")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .invalidConfiguration)
    }
    do {
      try await PresetDocumentFiles.writeAtomically(Data("unaccepted".utf8), to: remote)
      XCTFail("A remote URL must not replace a local file with the same path.")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .invalidConfiguration)
    }
    XCTAssertEqual(try Data(contentsOf: local), original)
  }

  func testNonLocalAuthorityCannotCreateTheSameLocalFile() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let local = root.appendingPathComponent("absent.preset")
    do {
      try await PresetDocumentFiles.writeAtomically(
        Data("unaccepted".utf8), to: withHost("storage.example.invalid", local))
      XCTFail("A non-local authority must not be discarded before creating a file.")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .invalidConfiguration)
    }
    XCTAssertFalse(FileManager.default.fileExists(atPath: local.path))
  }

  func testLocalAndLocalhostAuthoritiesRemainCompatible() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    for host in [nil, "", "localhost", "LOCALHOST"] as [String?] {
      let local = root.appendingPathComponent(UUID().uuidString + ".preset")
      let selected = try withHost(host, local)
      let bytes = Data("accepted".utf8)
      try await PresetDocumentFiles.writeAtomically(bytes, to: selected)
      let read = try await PresetDocumentFiles.read(from: selected)
      XCTAssertEqual(read, bytes)
      XCTAssertEqual(try Data(contentsOf: local), bytes)
    }
  }

  func testUnsupportedLocalhostAuthorityCannotReplaceTheSameLocalFile() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let local = root.appendingPathComponent("reviewed.preset")
    let original = Data("reviewed".utf8)
    try original.write(to: local)
    for userInfo in [false, true] {
      var components = try XCTUnwrap(URLComponents(url: local, resolvingAgainstBaseURL: false))
      components.host = "localhost"
      if userInfo { components.user = "fixture-user" } else { components.port = 12345 }
      do {
        try await PresetDocumentFiles.writeAtomically(
          Data("unaccepted".utf8), to: XCTUnwrap(components.url))
        XCTFail("Native paths cannot represent a file URL's user information or port.")
      } catch let error as PresetDocumentError {
        XCTAssertEqual(error, .invalidConfiguration)
      }
      XCTAssertEqual(try Data(contentsOf: local), original)
    }
  }

  func testAmbiguousSpellingCannotSelectAFileForWriteOrApproval() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: root) }
    let expected = Data("reviewed".utf8)
    for (name, spelling) in [
      ("reviewed.preset", "reviewed.preset/"),
      ("reviewed.preset", "reviewed.preset?unused"),
      ("reviewed.preset", "reviewed.preset#unused"),
      ("%00.preset", "%00.preset"),
      ("%2F.preset", "%2F.preset"),
    ] {
      let file = root.appendingPathComponent(name, isDirectory: false)
      try expected.write(to: file)
      let selected = try XCTUnwrap(URL(string: root.absoluteString + "/" + spelling))
      do {
        try await PresetDocumentFiles.writeAtomically(Data("unaccepted".utf8), to: selected)
        XCTFail("An ambiguous URL must not replace a regular file.")
      } catch let error as PresetDocumentError {
        XCTAssertEqual(error, .invalidConfiguration)
      }
      do {
        _ = try await PresetDocumentFiles.captureReplacement(at: selected)
        XCTFail("An ambiguous URL must not capture a regular file.")
      } catch let error as PresetDocumentError {
        XCTAssertEqual(error, .invalidConfiguration)
      }
      let restored = try await PresetDocumentFiles.read(from: file)
      XCTAssertEqual(restored, expected)
    }
  }

  private func withHost(_ host: String?, _ url: URL) throws -> URL {
    var components = try XCTUnwrap(URLComponents(url: url, resolvingAgainstBaseURL: false))
    components.host = host
    return try XCTUnwrap(components.url)
  }
}
