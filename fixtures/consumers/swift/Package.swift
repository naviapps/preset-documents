// swift-tools-version: 6.3
import PackageDescription

let package = Package(
  name: "PresetDocumentsConsumer",
  platforms: [.macOS(.v26)],
  dependencies: [.package(name: "preset-documents", path: "../sdk")],
  targets: [
    .executableTarget(
      name: "PresetDocumentsConsumer",
      dependencies: [
        .product(name: "PresetDocuments", package: "preset-documents"),
        .product(name: "PresetDocumentsFileIO", package: "preset-documents"),
      ])
  ]
)
