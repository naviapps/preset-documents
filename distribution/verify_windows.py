#!/usr/bin/env python3
"""Validate the Windows x64 SDK and clean NuGet consumption on Windows."""

import argparse
import json
import platform
import shutil
import tempfile
from pathlib import Path

from sdk_tooling import windows_delivery as delivery
from sdk_tooling.adapters import load, support_inputs
from sdk_tooling.native_plan import ffi_product
from sdk_tooling.windows_delivery import RID, TARGET, X64Acceptance

from distribution.pack_nuget import pack
from distribution.verify_bindings import consume

ROOT = Path(__file__).resolve().parents[1]
RECEIPT = "preset-documents-windows.json"
NATIVE_RECEIPT = "native-build.json"


def export_native(built, inventory, artifact_root, tooling):
    destination = artifact_root / "sdk-windows" / RID
    destination.parent.mkdir(parents=True, exist_ok=True)
    if (
        destination.exists()
        or destination.is_symlink()
        or not destination.parent.resolve().is_relative_to(artifact_root.resolve())
    ):
        raise ValueError("Windows native export requires a new directory inside the artifact root")
    with tempfile.TemporaryDirectory(dir=destination.parent, prefix="native-export-") as temporary:
        stage = Path(temporary) / RID
        shutil.copytree(built, stage, symlinks=True)
        if tooling.hashes(tooling.files(stage), stage) != inventory:
            raise ValueError("Exported Windows native files differ from the accepted build")
        stage.rename(destination)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument(
        "--export-native",
        action="store_true",
        help="Export the accepted Windows build for a combined NuGet package",
    )
    args = parser.parse_args(argv)
    artifact_root = load("vm_environment").artifacts_dir()
    if args.export_native and artifact_root is None:
        parser.error("--export-native requires an owned artifact directory")
    source = delivery.acceptance_source(ROOT, match_actions=True)
    support = support_inputs(ROOT)
    windows = load("windows")
    artifact_tooling = load("artifacts")
    acceptance = X64Acceptance(ROOT)
    native = ffi_product(ROOT)

    built = acceptance.build(native)
    env, dotnet = acceptance.env, acceptance.dotnet
    output_dir = ROOT / ".native-build"
    output_dir.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output_dir, prefix="win-x64-candidate-") as directory:
        candidate = Path(directory)
        with artifact_tooling.build_lock(output_dir):
            dll, import_library, native_receipt = acceptance.stage(native, built, candidate)
            native_digest = artifact_tooling.file_sha256(built / NATIVE_RECEIPT)
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
        package = pack(native, (RID,), environment=dict(env, DOTNET_BIN=str(dotnet)))
        package_receipt = package.with_suffix(".nupkg.json")
        receipt_bytes = package_receipt.read_bytes()
        packaged = json.loads(receipt_bytes)
        package_hash = artifact_tooling.file_sha256(package)
        if packaged.get("sha256") != package_hash or packaged.get("nativeReceipts") != {
            RID: native_digest
        }:
            raise ValueError("Windows package differs from the accepted x64 build")
        consume(
            package,
            dotnet=dotnet,
            version=native.version,
            root=ROOT,
            env=env,
            output=candidate,
            exclude_paths=(ROOT / "target", built, candidate),
            runtime=RID,
        )
        if (
            package_receipt.read_bytes() != receipt_bytes
            or artifact_tooling.file_sha256(package) != package_hash
        ):
            raise ValueError("Windows package changed during installed consumer acceptance")
        dll_hash = native_receipt["outputs"][dll.name]
        if artifact_tooling.file_sha256(dll) != dll_hash:
            raise ValueError("Staged DLL changed during Windows acceptance")
        if source != delivery.acceptance_source(ROOT, match_actions=True):
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
        if args.export_native:
            with artifact_tooling.build_lock(output_dir):
                export_native(
                    built,
                    {**native_receipt["outputs"], NATIVE_RECEIPT: native_digest},
                    artifact_root,
                    artifact_tooling,
                )
    print(f"Windows x64 preset-documents acceptance passed on {acceptance.host}.")


if __name__ == "__main__":
    main()
