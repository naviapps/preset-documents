//! Opt-in approved replacement. A committed exchange never deletes the displaced item.
use preset_documents::Error;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    device: i32,
    inode: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Revision {
    identity: Identity,
    size: i64,
    change_seconds: i64,
    change_nanoseconds: i64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

/// A process-local observation of a regular file at a specific UTF-8 path.
/// Keep it only for the approval flow that requested it; it is not an OS lock or permission token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplacementRevision {
    path: String,
    parent: Identity,
    file: Revision,
}

impl ReplacementRevision {
    /// The exact UTF-8 path inspected when the revision was captured.
    pub fn captured_path(&self) -> &str {
        &self.path
    }
}

/// Identity observed after an exchange. Even a matching inode can have been edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplacedIdentity {
    MatchesApprovedFile,
    Different,
    Unverified,
}

/// Result of an attempted native exchange whose recovery directory was retained.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ReplacementOutcome {
    /// The exchange committed. `displaced-preset` is the prior item, not necessarily the approved revision.
    Committed {
        recovery_directory: String,
        displaced_identity: DisplacedIdentity,
    },
    /// The exchange did not return a definitive result. No item in the recovery directory was removed.
    CommitUncertain { recovery_directory: String },
}

/// Captures a regular replacement destination, without reading its contents or creating any files.
/// Currently supported only on local macOS APFS. Missing files and final links are rejected.
pub fn capture_replacement(path: &Path) -> Result<ReplacementRevision, Error> {
    #[cfg(target_os = "macos")]
    {
        macos::capture(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(Error::ApprovedReplacementUnsupported)
    }
}

/// Rechecks the captured path and revision before staging and immediately before exchange.
/// The exchange cannot create a missing destination, but it does not compare identity atomically.
/// Every committed exchange retains the displaced item under `displaced-preset` in the private
/// sibling recovery directory. Do not delete it automatically, including after a matching identity.
/// The directory path is the last observed location, and the prior item is not an immutable copy.
pub fn replace_approved(
    path: &Path,
    revision: &ReplacementRevision,
    bytes: &[u8],
    maximum: usize,
) -> Result<ReplacementOutcome, Error> {
    #[cfg(target_os = "macos")]
    {
        macos::replace(path, revision, bytes, maximum)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, revision, bytes, maximum);
        Err(Error::ApprovedReplacementUnsupported)
    }
}
