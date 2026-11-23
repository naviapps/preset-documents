#!/usr/bin/env python3
"""Validate the Windows x64 SDK and clean NuGet consumption on Windows."""

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import tempfile
import tomllib
from contextlib import ExitStack
from pathlib import Path

from _tooling import load_candidate
from native import builder

from bindings import pack_and_consume

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
RUNTIME = "win-x64"
RECEIPT = "preset-documents-windows.json"


def acceptance_source():
    commit = os.environ.get("NAVI_CI_SOURCE_COMMIT")
    fingerprint = os.environ.get("NAVI_CI_SOURCE_FINGERPRINT")
    if commit is not None or fingerprint is not None:
        if not re.fullmatch(r"[a-f0-9]{64}", fingerprint or ""):
            raise ValueError("Captured Windows x64 acceptance requires the workspace SHA-256")
    else:
        if subprocess.check_output(
            ["git", "status", "--porcelain=v1", "--untracked-files=all"], cwd=ROOT, text=True
        ).strip():
            raise ValueError("Windows x64 acceptance requires a clean committed checkout")
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if not re.fullmatch(r"[a-f0-9]{40}(?:[a-f0-9]{24})?", commit or ""):
        raise ValueError("Windows x64 acceptance requires the full source commit SHA")
    return commit, fingerprint


def publish_x64(package, receipt, *, output_dir, artifact_root, tooling):
    evidence_path = Path("win-x64") / receipt["version"]
    local = output_dir / evidence_path
    external = artifact_root / evidence_path if artifact_root else None
    local_path = local.resolve()
    external_path = external.resolve() if external is not None else local_path
    destinations = [local]
    if external_path != local_path:
        if external_path.is_relative_to(local_path) or local_path.is_relative_to(external_path):
            raise ValueError("Local and external x64 evidence directories must be disjoint")
        destinations.append(external)

    names = {package.name, RECEIPT}
    digest = receipt["packages"][package.name]

    def verify(directory):
        if (
            {entry.name for entry in directory.iterdir()} != names
            or json.loads((directory / RECEIPT).read_bytes()) != receipt
            or hashlib.sha256((directory / package.name).read_bytes()).hexdigest() != digest
        ):
            raise ValueError("Windows x64 evidence differs from the accepted package and receipt")

    def check_destination(destination):
        if (
            destination.parent.is_symlink()
            or destination.parent.is_junction()
            or destination.is_symlink()
            or destination.is_junction()
            or (
                destination.exists()
                and (
                    not destination.is_dir()
                    or any(
                        entry.name not in names or not entry.is_file() or entry.is_symlink()
                        for entry in destination.iterdir()
                    )
                )
            )
        ):
            raise ValueError("Windows x64 evidence directory contains unrelated files")

    with tooling.build_lock(output_dir), ExitStack() as temporary:
        for destination in destinations:
            check_destination(destination)
        stages = []
        for destination in destinations:
            destination.parent.mkdir(parents=True, exist_ok=True)
            path = temporary.enter_context(
                tempfile.TemporaryDirectory(dir=destination.parent, prefix="evidence-")
            )
            stage = Path(path) / destination.name
            stage.mkdir()
            shutil.copyfile(package, stage / package.name)
            tooling.write_json(stage / RECEIPT, receipt)
            verify(stage)
            stages.append(stage)

        cleanup_failure = None

        def verify_local():
            nonlocal cleanup_failure
            verify(local)
            if len(stages) == 1:
                return
            check_destination(external)
            external_verified = False

            def verify_external():
                nonlocal external_verified
                verify(external)
                external_verified = True

            # Keep the local rollback available until the external pair verifies.
            try:
                tooling.publish(stages[1], external, verify_external)
            except BaseException as error:
                if not external_verified:
                    raise
                # The helper cleans its previous tree after verification. Preserve both
                # verified outputs if that cleanup failed.
                verify(external)
                cleanup_failure = error

        check_destination(local)
        tooling.publish(stages[0], local, verify_local)
        if cleanup_failure is not None:
            raise cleanup_failure


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--architecture", choices=("x64",), default="x64")
    parser.add_argument(
        "--tooling-source", type=Path, required=True, help="Local reviewed SDK tooling checkout"
    )
    parser.add_argument(
        "--tooling-revision", required=True, help="Exact Git commit of the local SDK tooling"
    )
    args = parser.parse_args(argv)
    source = acceptance_source()
    selected = load_candidate(args.tooling_source, args.tooling_revision)
    windows = selected("windows")
    bindings = selected("bindings")
    artifact_tooling = selected("artifacts")
    environment = {
        key.upper(): value for key, value in os.environ.items() if key.upper() != "DOTNET_HOST_PATH"
    }
    host = windows.host_architecture()
    env, tools, dotnet = windows.native_environment(host, environment, target_architecture="x64")
    env["RUSTUP_AUTO_INSTALL"] = "0"
    dotnet_version = subprocess.check_output(
        [str(dotnet), "--version"], cwd=ROOT, env=env, text=True
    ).strip()
    if dotnet_version != tomllib.loads((ROOT / "mise.toml").read_text())["tools"]["dotnet"]:
        raise ValueError("Use the pinned x64 .NET SDK version from mise.toml")
    native = builder(selected)

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
            dll = bindings.stage_native(dll, candidate / dll.name)
            import_library = bindings.stage_native(import_library, candidate / import_library.name)
        expected = windows.verify_library(
            dll,
            [ROOT / "crates/ffi/include/preset_documents.h"],
            "preset_documents_",
            tools,
            env,
            "x64",
        )
        bindings.compile_c(
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
        bindings.run_managed(
            dotnet,
            ROOT / "bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj",
            cwd=ROOT,
            env=env,
            verify_style=True,
            runtime=RUNTIME,
            properties={"UseAppHost": "false"},
        )
        package, package_hash = pack_and_consume(
            bindings,
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
        if source != acceptance_source():
            raise ValueError("Windows x64 acceptance source changed")
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
            "sdkToolingCommit": args.tooling_revision,
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
        artifact_root = Path(env["NAVI_CI_ARTIFACTS"]) if env.get("NAVI_CI_ARTIFACTS") else None
        publish_x64(
            package,
            receipt,
            output_dir=output_dir,
            artifact_root=artifact_root,
            tooling=artifact_tooling,
        )
    print(f"Windows x64 preset-documents acceptance passed on {host}.")


if __name__ == "__main__":
    main()
