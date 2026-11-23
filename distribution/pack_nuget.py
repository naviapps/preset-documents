#!/usr/bin/env python3
"""Pack verified PresetDocuments runtime assets for local or CI use."""

import argparse
from pathlib import Path

from sdk_tooling.adapters import load
from sdk_tooling.native_plan import ffi_product
from sdk_tooling.windows_delivery import RID
from sdk_tooling.xcframework import xcframework

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "NaviApps.PresetDocuments"
MACOS_RUNTIMES = ("osx-arm64", "osx-x64")


def pack(native, runtimes, *, environment=None):
    root = native.root
    nuget = load("nuget")
    package_files = {"LICENSE": root / "LICENSE", "README.md": root / "README.md"}
    package_files.update(nuget.retained_notice_files(root / "ThirdParty", "licenses"))
    return nuget.pack_native(
        native,
        project=root / "bindings/dotnet/src" / PACKAGE / (PACKAGE + ".csproj"),
        package_files=package_files,
        nuget_config=root / "bindings/dotnet/NuGet.Config",
        input_files=[
            root / name for name in ("distribution/pack_nuget.py", "native.toml", "mise.toml")
        ],
        runtimes=runtimes,
        environment=environment,
    )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--runtime", action="append", required=True, choices=(*MACOS_RUNTIMES, RID))
    parser.add_argument(
        "--result", type=Path, help="Write the accepted package filename to a new file"
    )
    args = parser.parse_args(argv)
    native = ffi_product(ROOT) if set(args.runtime) == {RID} else xcframework(ROOT)
    package = pack(native, args.runtime)
    if args.result is not None:
        with args.result.open("x", encoding="utf-8") as output:
            output.write(package.name)


if __name__ == "__main__":
    main()
