# ``PresetDocumentsFileIO``

Bounded file reads and atomic publication for host-selected preset files.

Select the `PresetDocumentsFileIO` Swift product when using these APIs.
Hosts own file selection, security-scoped access, import
policy, user confirmation and recovery UI. The codec and file products share one
native framework; the product boundary does not remove native filesystem dependencies.

## Reading and publishing files

- ``PresetDocumentFiles/read(from:maximumByteCount:)`` reads a regular file within
  the byte budget, including growth after opening it.
- ``PresetDocumentFiles/writeNew(_:to:maximumByteCount:)`` creates a complete new
  file without replacing an occupied name.
- ``PresetDocumentFiles/writeAtomically(_:to:maximumByteCount:)`` replaces a
  regular file or creates an absent file. For an existing file, it rechecks
  its revision separately before renaming. An initially absent file uses
  exclusive creation, preserving a competing file and reporting a destination
  change. If the volume lacks exclusive publication, creating a file reports
  `atomicNewUnsupported`.

File operations require local file URLs and never create parent directories.
The default file byte limit is 64 MiB. A committed write keeps its actual result
after late cancellation. The
[shared file contract](https://github.com/naviapps/preset-documents#optional-file-io)
defines supported paths and volumes, typed failures, Finder tag preservation
and durability limits for all bindings.

## Replacing a user-approved revision

For a host confirmation that must not turn into a new-file write, use
``PresetDocumentFiles/captureReplacement(at:)`` before asking for approval and
``PresetDocumentFiles/replaceApproved(_:with:maximumByteCount:)`` only after approval.
This is supported on macOS APFS; unsupported destinations report
`approvedReplacementUnsupported`. A missing or observed changed destination
reports `destinationChanged`. The operation never creates a missing destination.

Every committed exchange retains its displaced item at the outcome's
``PresetReplacementOutcome/displacedPresetURL``. The revision check and exchange
are separate: even a matching displaced identity does not establish unchanged
contents. Present ``PresetReplacementOutcome/recoveryDirectory`` to the user;
do not automatically remove recovery or retry an uncertain commit. The shared
file contract describes the retained entry and uncertain-result limits.

```swift
import Foundation
import PresetDocumentsFileIO

func exportReplacingApprovedFile(
  _ data: Data, at destination: URL,
  confirm: (URL) async -> Bool
) async throws -> PresetReplacementOutcome? {
  let revision = try await PresetDocumentFiles.captureReplacement(at: destination)
  guard await confirm(destination) else { return nil }
  return try await PresetDocumentFiles.replaceApproved(revision, with: data)
}
```

## Topics

### File operations

- ``PresetDocumentFiles``

### Approved replacement and recovery

- ``PresetReplacementRevision``
- ``PresetReplacementOutcome``
