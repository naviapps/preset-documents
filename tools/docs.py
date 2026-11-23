#!/usr/bin/env python3
"""Validate this SDK's public Swift guide against its owned symbols."""

from pathlib import Path

from _tooling import load


def main():
    root = Path(__file__).resolve().parents[1]
    load("docs").check_catalogs(
        root,
        sorted((root / "Sources").rglob("*.docc")),
        output=root / "build/docs",
        bundle_identifier_prefix="com.naviapps.",
    )


if __name__ == "__main__":
    main()
