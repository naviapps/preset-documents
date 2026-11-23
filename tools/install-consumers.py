#!/usr/bin/env python3
"""Run the SDK's independent Git Rust and installed macOS C fixtures."""

import json
from pathlib import Path

from _tooling import load

ROOT = Path(__file__).resolve().parent.parent


def _file_arguments(client, owned):
    options = client / "file-options.json"
    options.write_text(
        json.dumps({"path": str(owned / "c-consumer.preset"), "maximumByteCount": 4096}),
        encoding="utf-8",
    )
    return (options,)


def main(argv=None):
    load("installed_consumers").check_installed(
        root=ROOT,
        git_url="https://github.com/naviapps/preset-documents.git",
        rust_packages=("preset-documents", "preset-documents-file-io"),
        framework=(
            "Artifacts/CPresetDocuments.xcframework/macos-arm64_x86_64/CPresetDocuments.framework"
        ),
        header="preset_documents.h",
        c_arguments=_file_arguments,
        argv=argv,
    )


if __name__ == "__main__":
    main()
