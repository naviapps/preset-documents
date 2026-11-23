#!/usr/bin/env python3
"""Validate C ownership and managed contracts against the native macOS library."""

import os
import shutil
import subprocess
import sys
from pathlib import Path

from _tooling import load
from native import builder

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = "NaviApps.PresetDocuments"


def pack_and_consume(
    tooling,
    dotnet,
    library,
    *,
    rid,
    version,
    cwd,
    env,
    output,
    exclude_paths=(),
    consumer_runtime=None,
):
    filename = "libpreset_documents.dylib" if rid == "osx" else library.name
    staged = tooling.stage_native(library, output / "native" / rid / filename)
    expected_library = library
    if rid == "osx":
        # The framework signature covers its Info.plist and resources. The detached
        # dylib needs its own signature, while the accepted framework stays intact.
        subprocess.run(
            ["codesign", "--force", "--sign", "-", "--timestamp=none", str(staged)],
            cwd=cwd,
            env=env,
            check=True,
        )
        subprocess.run(
            ["codesign", "--verify", "--strict", "--all-architectures", str(staged)],
            cwd=cwd,
            env=env,
            check=True,
        )
        expected_library = staged
    packages = output / "packages"
    packages.mkdir(parents=True, exist_ok=True)
    project = cwd / "bindings/dotnet/src" / PACKAGE / (PACKAGE + ".csproj")
    tooling.pack_project(
        dotnet,
        project,
        output=packages,
        cwd=cwd,
        env=env,
        properties={
            "Version": version,
            "PresetDocumentsRuntimeIdentifier": rid,
            "PresetDocumentsNativeLibrary": str(staged),
        },
    )
    package = packages / f"{PACKAGE}.{version}.nupkg"
    notices = {
        "licenses/" + path.relative_to(cwd / "ThirdParty").as_posix(): path
        for path in (cwd / "ThirdParty").rglob("*")
        if path.is_file() and path.name not in (".DS_Store", "registry-notices.json")
    }
    package_hash = tooling.audit_package(
        package,
        expected_files={"LICENSE": cwd / "LICENSE", "README.md": cwd / "README.md", **notices},
        runtime_files={f"runtimes/{rid}/native/{filename}": expected_library},
    )
    clear_environment = ("PRESET_DOCUMENTS_NATIVE_LIBRARY",)
    if rid == "osx":
        clear_environment += (
            "DYLD_LIBRARY_PATH",
            "DYLD_FALLBACK_LIBRARY_PATH",
            "DYLD_FRAMEWORK_PATH",
        )
    tooling.run_isolated_consumer(
        dotnet,
        packages=[package],
        references={PACKAGE: version},
        source=(cwd / "fixtures/dotnet-consumer/Program.cs").read_text(encoding="utf-8"),
        work_parent=output,
        env=env,
        clear_environment=clear_environment,
        exclude_paths=(cwd / "target", library.parent, output / "native", *exclude_paths),
        runtime=consumer_runtime,
    )
    return package, package_hash


def main():
    if sys.platform != "darwin":
        raise SystemExit("Use tools/windows.py in the owned Windows worker")
    dotnet = os.environ.get("DOTNET_BIN") or shutil.which("dotnet")
    if not dotnet:
        raise SystemExit("Set DOTNET_BIN to the task's .NET 10 executable")
    output = ROOT / ".native-build" / "macos"
    output.mkdir(parents=True, exist_ok=True)
    native = builder()
    native.verify()
    framework_dir = native.artifact / "macos-arm64_x86_64"
    library = framework_dir / (native.module + ".framework") / native.module
    bindings = load("bindings")
    bindings.compile_c(
        ROOT / "crates/ffi/tests/c/abi.c",
        output=output / "abi",
        compiler=["xcrun", "clang"],
        env=os.environ.copy(),
        cwd=ROOT,
        include_dirs=[ROOT / "crates/ffi/include"],
        flags=[
            "-F",
            str(framework_dir),
            "-framework",
            native.module,
            "-Wl,-rpath," + str(framework_dir),
        ],
    )
    subprocess.run([str(output / "abi")], cwd=ROOT, check=True)
    env = dict(
        os.environ,
        PRESET_DOCUMENTS_NATIVE_LIBRARY=str(library),
        DOTNET_CLI_TELEMETRY_OPTOUT="1",
        DOTNET_NOLOGO="1",
    )
    bindings.run_managed(
        dotnet,
        ROOT / "bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj",
        cwd=ROOT,
        env=env,
        verify_style=True,
    )
    pack_and_consume(
        bindings,
        dotnet,
        library,
        rid="osx",
        version=native.version,
        cwd=ROOT,
        env=env,
        output=output,
    )
    print("C ownership, managed contracts and clean macOS NuGet acceptance passed.")


if __name__ == "__main__":
    main()
