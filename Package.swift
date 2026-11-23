// swift-tools-version: 6.4
import PackageDescription

let package = Package(
  name: "PresetDocuments",
  platforms: [.macOS(.v26)],
  products: [
    .library(name: "PresetDocuments", targets: ["PresetDocuments"]),
    .library(name: "PresetDocumentsFileIO", targets: ["PresetDocumentsFileIO"]),
  ],
  targets: [
    .binaryTarget(name: "CPresetDocuments", path: "Artifacts/CPresetDocuments.xcframework"),
    .target(name: "PresetDocuments", dependencies: ["CPresetDocuments"]),
    .target(name: "PresetDocumentsFileIO", dependencies: ["PresetDocuments"]),
    .testTarget(
      name: "PresetDocumentsTests", dependencies: ["PresetDocuments", "PresetDocumentsFileIO"]),
  ]
)
