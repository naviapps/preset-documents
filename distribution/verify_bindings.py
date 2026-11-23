#!/usr/bin/env python3
"""Validate C ownership and managed contracts against the native macOS library."""

import os
import shutil
import subprocess
import sys

from sdk_tooling.adapters import load
from sdk_tooling.xcframework import xcframework

from distribution.pack_nuget import MACOS_RUNTIMES, PACKAGE, ROOT, pack


def consume(package, *, dotnet, version, root, env, output, exclude_paths, runtime=None):
    load("nuget").run_isolated_consumer(
        dotnet,
        packages=[package],
        references={PACKAGE: version},
        source=(root / "fixtures/consumers/dotnet/Program.cs").read_text(encoding="utf-8"),
        work_parent=output,
        env=env,
        exclude_paths=exclude_paths,
        runtime=runtime,
        properties={"TreatWarningsAsErrors": "true"},
    )


def main():
    if sys.platform != "darwin":
        raise SystemExit("Run distribution/verify_windows.py in the owned Windows worker")
    dotnet = os.environ.get("DOTNET_BIN") or shutil.which("dotnet")
    if not dotnet:
        raise SystemExit("Set DOTNET_BIN to the task's .NET 10 executable")
    output = ROOT / ".native-build" / "macos"
    output.mkdir(parents=True, exist_ok=True)
    native = xcframework(ROOT)
    native.verify()
    framework_dir = native.artifact / "macos-arm64_x86_64"
    library = framework_dir / (native.module + ".framework") / native.module
    load("c_compiler").compile_c(
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
        DOTNET_CLI_TELEMETRY_OPTOUT="1",
        DOTNET_NOLOGO="1",
    )
    load("dotnet").run_managed(
        dotnet,
        ROOT / "bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj",
        cwd=ROOT,
        env=env,
        properties={"PresetDocumentsNativeLibrary": str(library)},
        verify_style=True,
    )
    package = pack(native, MACOS_RUNTIMES, environment=dict(env, DOTNET_BIN=dotnet))
    consume(
        package,
        dotnet=dotnet,
        version=native.version,
        root=ROOT,
        env=env,
        output=output,
        exclude_paths=(ROOT / "target", library.parent),
    )
    print("C ownership, managed contracts and clean macOS NuGet acceptance passed.")


if __name__ == "__main__":
    main()
