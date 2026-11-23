# SDK development

Keep shared values, validation, deterministic planning and state in Rust. Keep native resources and language scheduling in their adapters. Hosts own product policy, permission prompts, persistence and UI.

Use Git on main. Preserve published commits and pinned dependencies; keep unfinished edits in the working tree and publish coherent main units without PRs.

Swift consumers install through Package.swift and tracked native frameworks. Rebuild native artifacts after Rust/C/input changes; maintain their provenance and runtime notices. Keep C ownership explicit and copy foreign views before invoking host callbacks. Test OS effects only in owned isolated fixtures and report actual runtime coverage separately from compilation.

Use English documentation/comments. Keep `make check` and public GitHub-hosted CI
usable from a fresh SDK clone with the public SDK Tooling pin in `mise.toml`. Follow the
[distribution checks](README.md#distribution-checks) for release verification.
Human QA/data and existing VM/security/sharing settings are protected. Read
README.md and the affected public API comments before changing a contract.
