#!/usr/bin/env python3
"""Validate the Windows x64 SDK and clean NuGet consumption on Windows."""

import argparse
import platform
import tempfile
from pathlib import Path

from sdk_tooling import windows_delivery as delivery
from sdk_tooling.adapters import load, support_inputs
from sdk_tooling.native_plan import ffi_product
from sdk_tooling.windows_delivery import RID, TARGET, X64Acceptance

from distribution.verify_bindings import pack_and_consume

ROOT = Path(__file__).resolve().parents[1]
RECEIPT = "preset-documents-windows.json"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)
    artifact_root = load("vm_environment").artifacts_dir()
    source = delivery.acceptance_source(ROOT)
    support = support_inputs(ROOT)
    windows = load("windows")
    artifact_tooling = load("artifacts")
    acceptance = X64Acceptance(ROOT)
    native = ffi_product(ROOT)

    # The native build verifies retained notices for the macOS and Windows graphs.
    built = acceptance.build(native, fetch_all=True)
    env, dotnet = acceptance.env, acceptance.dotnet
    output_dir = ROOT / ".native-build"
    output_dir.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output_dir, prefix="win-x64-candidate-") as directory:
        candidate = Path(directory)
        with artifact_tooling.build_lock(output_dir):
            dll, import_library, native_receipt = acceptance.stage(native, built, candidate)
        expected = windows.verify_library(
            dll,
            [ROOT / "crates/ffi/include/preset_documents.h"],
            "preset_documents_",
            acceptance.tools,
            env,
            "x64",
        )
        acceptance.run_c(
            ROOT / "crates/ffi/tests/c/abi.c",
            include_directory=ROOT / "crates/ffi/include",
            import_library=import_library,
        )
        load("dotnet").run_managed(
            dotnet,
            ROOT / "bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj",
            cwd=ROOT,
            env=env,
            verify_style=True,
            runtime=RID,
            properties={"PresetDocumentsNativeLibrary": str(dll)},
        )
        package, package_hash = pack_and_consume(
            load,
            dotnet,
            dll,
            rid=RID,
            version=native.version,
            cwd=ROOT,
            env=env,
            output=candidate,
            exclude_paths=(built,),
            consumer_runtime=RID,
        )
        dll_hash = native_receipt["outputs"][dll.name]
        if artifact_tooling.file_sha256(dll) != dll_hash:
            raise ValueError("Staged DLL changed during Windows acceptance")
        if source != delivery.acceptance_source(ROOT):
            raise ValueError("Windows x64 acceptance source changed")
        if support != support_inputs(ROOT):
            raise ValueError("Windows x64 acceptance support files changed")
        commit, fingerprint = source
        receipt = {
            "source": commit,
            "workspaceFingerprint": fingerprint,
            "target": TARGET,
            "runtime": RID,
            "os": platform.platform(),
            "exports": sorted(expected),
            "version": native.version,
            "toolchain": acceptance.toolchain,
            "toolInputs": support,
            "dotnetVersion": acceptance.dotnet_version,
            "execution": "x64-emulated-on-arm64" if acceptance.host == "arm64" else "x64-native",
            "run": env.get("GITHUB_RUN_ID"),
            "attempt": env.get("GITHUB_RUN_ATTEMPT"),
            "acceptance": [
                "Rust document/format/Unicode/limits",
                "C ownership",
                ".NET document contracts",
                "clean NuGet consumer",
            ],
            "packages": {package.name: package_hash},
            "dllSha256": dll_hash,
        }
        delivery.publish_x64(
            package,
            receipt,
            receipt_name=RECEIPT,
            output_dir=output_dir,
            artifact_root=artifact_root,
            tooling=artifact_tooling,
        )
    print(f"Windows x64 preset-documents acceptance passed on {acceptance.host}.")


if __name__ == "__main__":
    main()
