import Foundation
import PresetDocuments
import PresetDocumentsFileIO
import XCTest

final class PresetDocumentFilesTests: XCTestCase {
  func testWriteNewRoundTripsAnOpaquePresetAtAUnicodePathAndMapsCollision() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("日本語.preset")
    let original = Data([0, 42, 255])
    try await PresetDocumentFiles.writeNew(original, to: file, maximumByteCount: original.count)

    do {
      try await PresetDocumentFiles.writeNew(Data("competing preset".utf8), to: file)
      XCTFail("Expected destination collision")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .destinationAlreadyExists)
    }
    let actual = try await PresetDocumentFiles.read(from: file, maximumByteCount: original.count)
    XCTAssertEqual(actual, original)
  }

  func testWriteNewRejectsBudgetAndCancellationWithoutCreatingTheDestination() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("new.preset")
    let data = Data("new".utf8)
    do {
      try await PresetDocumentFiles.writeNew(data, to: file, maximumByteCount: 2)
      XCTFail("Expected output budget rejection")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .outputTooLarge(byteCount: data.count, maximumByteCount: 2))
    }
    let writer = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      try await PresetDocumentFiles.writeNew(data, to: file)
    }
    do {
      try await writer.value
      XCTFail("Expected pre-dispatch cancellation")
    } catch is CancellationError {}
    XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: root.path), [])
  }

  func testReadAndAtomicWriteMapTypedLimitsAndPreserveBytesOnCancellation() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("日本語.preset")
    try await PresetDocumentFiles.writeAtomically(Data("old".utf8), to: file, maximumByteCount: 3)
    do {
      try await PresetDocumentFiles.writeAtomically(
        Data("excess".utf8), to: file, maximumByteCount: 3)
      XCTFail("Expected output budget rejection")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .outputTooLarge(byteCount: 6, maximumByteCount: 3))
    }
    let previous = try await PresetDocumentFiles.read(from: file, maximumByteCount: 3)
    XCTAssertEqual(previous, Data("old".utf8))
    do {
      _ = try await PresetDocumentFiles.read(from: file, maximumByteCount: 2)
      XCTFail("Expected bounded read rejection")
    } catch let error as PresetDocumentError {
      XCTAssertEqual(error, .inputTooLarge(byteCount: 3, maximumByteCount: 2))
    }
    try await PresetDocumentFiles.writeAtomically(Data("new".utf8), to: file)
    XCTAssertEqual(try Data(contentsOf: file), Data("new".utf8))
    let writer = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      try await PresetDocumentFiles.writeAtomically(Data("cancelled".utf8), to: file)
    }
    do {
      try await writer.value
      XCTFail("Expected pre-dispatch cancellation")
    } catch is CancellationError {}
    XCTAssertEqual(try Data(contentsOf: file), Data("new".utf8))
  }
}
