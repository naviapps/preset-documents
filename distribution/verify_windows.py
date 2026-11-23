#!/usr/bin/env python3
"""Validate the Windows x64 SDK and clean NuGet consumption on Windows."""

import argparse
import hashlib
import os
import platform
import subprocess
import tempfile
import tomllib
from pathlib import Path

from sdk_tooling.adapters import load, support_inputs
from sdk_tooling.native_plan import builder

from distribution.verify_bindings import pack_and_consume

ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-msvc"
RUNTIME = "win-x64"
RECEIPT = "preset-documents-windows.json"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)
    environment = {
        key.upper(): value for key, value in os.environ.items() if key.upper() != "DOTNET_HOST_PATH"
    }
    artifact_root = load("vm_environment").artifacts_dir(environment)
    delivery = load("windows_delivery")
    source = delivery.acceptance_source(ROOT)
    support = support_inputs(ROOT)
    windows = load("windows")
    artifact_tooling = load("artifacts")
    host = windows.host_architecture()
    env, tools, dotnet = windows.native_environment(host, environment, target_architecture="x64")
    env["RUSTUP_AUTO_INSTALL"] = "0"
    dotnet_version = subprocess.check_output(
        [str(dotnet), "--version"], cwd=ROOT, env=env, text=True
    ).strip()
    if dotnet_version != tomllib.loads((ROOT / "mise.toml").read_text())["tools"]["dotnet"]:
        raise ValueError("Use the pinned x64 .NET SDK version from mise.toml")
    native = builder(ROOT)

    def run(*args):
        subprocess.run([str(a) for a in args], cwd=ROOT, env=env, check=True)

    # The native build verifies retained notices for the macOS and Windows graphs.
    run("cargo", "fetch", "--locked")
    run("cargo", "fmt", "--all", "--", "--check")
    built = windows.build_library(native, env, tools, "x64", native_host=host)
    run(
        "cargo",
        "clippy",
        "--workspace",
        "--all-targets",
        "--target",
        TARGET,
        "--locked",
        "--offline",
        "--",
        "-D",
        "warnings",
    )
    run("cargo", "test", "--workspace", "--target", TARGET, "--locked", "--offline")
    dll = built / "preset_documents.dll"
    import_library = built / "preset_documents.dll.lib"
    output_dir = ROOT / ".native-build"
    output_dir.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output_dir, prefix="win-x64-candidate-") as directory:
        candidate = Path(directory)
        executable = candidate / "abi.exe"
        toolchain = {"backend": "msvc", "host": windows.MSVC_TARGETS[host], "target": TARGET}
        # Windows searches the executable directory before PATH.
        with artifact_tooling.build_lock(output_dir):
            verified, native_receipt = windows.verify_artifact(native, "x64", environment=env)
            if verified != built or native_receipt.get("toolchain") != toolchain:
                raise ValueError("Windows x64 library differs from the selected host build")
            dll = artifact_tooling.stage_native(dll, candidate / dll.name)
            import_library = artifact_tooling.stage_native(
                import_library, candidate / import_library.name
            )
        expected = windows.verify_library(
            dll,
            [ROOT / "crates/ffi/include/preset_documents.h"],
            "preset_documents_",
            tools,
            env,
            "x64",
        )
        load("c_compiler").compile_c(
            ROOT / "crates/ffi/tests/c/abi.c",
            output=executable,
            compiler=[tools / "cl.exe"],
            include_dirs=[ROOT / "crates/ffi/include"],
            libraries=[import_library],
            style="msvc",
            cwd=ROOT,
            env=env,
        )
        windows.require_pe_machine(executable, "x64")
        run(executable)
        env["PRESET_DOCUMENTS_NATIVE_LIBRARY"] = str(dll)
        load("dotnet").run_managed(
            dotnet,
            ROOT / "bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj",
            cwd=ROOT,
            env=env,
            verify_style=True,
            runtime=RUNTIME,
            properties={"UseAppHost": "false"},
        )
        package, package_hash = pack_and_consume(
            load,
            dotnet,
            dll,
            rid=RUNTIME,
            version=native.version,
            cwd=ROOT,
            env=env,
            output=candidate,
            exclude_paths=(built,),
            consumer_runtime=RUNTIME,
        )
        if source != delivery.acceptance_source(ROOT):
            raise ValueError("Windows x64 acceptance source changed")
        if support != support_inputs(ROOT):
            raise ValueError("Windows x64 acceptance support files changed")
        commit, fingerprint = source
        receipt = {
            "source": commit,
            "workspaceFingerprint": fingerprint,
            "target": TARGET,
            "runtime": RUNTIME,
            "os": platform.platform(),
            "exports": sorted(expected),
            "version": native.version,
            "toolchain": toolchain,
            "toolInputs": support,
            "dotnetVersion": dotnet_version,
            "execution": "x64-emulated-on-arm64" if host == "arm64" else "x64-native",
            "run": env.get("GITHUB_RUN_ID"),
            "attempt": env.get("GITHUB_RUN_ATTEMPT"),
            "acceptance": [
                "Rust document/format/Unicode/limits",
                "C ownership",
                ".NET document contracts",
                "clean NuGet consumer",
            ],
            "packages": {package.name: package_hash},
            "dllSha256": hashlib.sha256(dll.read_bytes()).hexdigest(),
        }
        delivery.publish_x64(
            package,
            receipt,
            receipt_name=RECEIPT,
            output_dir=output_dir,
            artifact_root=artifact_root,
            tooling=artifact_tooling,
        )
    print(f"Windows x64 preset-documents acceptance passed on {host}.")


if __name__ == "__main__":
    main()
