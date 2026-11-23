"""SDK acceptance consumes the native builder's output, including redirected Cargo builds."""

import hashlib
import json
import os
import shutil
import tempfile
import unittest
from itertools import product
from pathlib import Path
from unittest.mock import Mock, patch

from sdk_tooling import windows_delivery
from sdk_tooling.adapters import load

from distribution import verify_windows as driver


class WindowsOutputTests(unittest.TestCase):
    def test_redirected_x64_build_cannot_consume_missing_or_stale_legacy_output(self):
        target = "x86_64-pc-windows-msvc"
        rid = "win-x64"
        hosts = {"arm64": "aarch64-pc-windows-msvc", "x64": target}
        for host, error, stale in product(
            hosts, (None, "receipt", "digest", "C machine", "support"), (False, True)
        ):
            with (
                self.subTest(host=host, error=error, stale_legacy_output=stale),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary)
                redirected = root / "redirected-cargo"
                published = root / ".native-build/accepted"
                published.mkdir(parents=True)
                library = published / "preset_documents.dll"
                library.write_bytes(b"fresh SDK DLL")
                import_library = published / (library.name + ".lib")
                import_library.write_bytes(b"fresh import library")
                fixture = root / "fixtures/consumers/dotnet/Program.cs"
                fixture.parent.mkdir(parents=True)
                fixture.write_text("using NaviApps.PresetDocuments;", encoding="utf-8")
                (root / "mise.toml").write_text('[tools]\ndotnet = "10.0.403"\n')
                legacy = root / "target" / target / "release"
                if stale:
                    legacy.mkdir(parents=True)
                    (legacy / library.name).write_bytes(b"stale SDK DLL")
                artifacts = root / "accepted-evidence"
                env = {
                    "CARGO_TARGET_DIR": str(redirected),
                    "PATH": "owned-toolchain",
                }
                dotnet = root / "dotnet-x64/dotnet.exe"
                native = Mock(version="0.1.0", library="preset_documents")
                shared = Mock()
                shared.host_architecture.return_value = host
                shared.MSVC_TARGETS = hosts
                shared.native_environment.return_value = (env, root / "msvc", dotnet)
                shared.build_library.return_value = published
                toolchain = {
                    "backend": "msvc",
                    "host": hosts[host],
                    "target": target,
                }
                shared.verify_artifact.return_value = (
                    published,
                    {
                        "toolchain": {} if error == "receipt" else toolchain,
                        "outputs": {
                            path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                            for path in (library, import_library)
                        },
                    },
                )
                if error == "C machine":
                    shared.require_pe_machine.side_effect = ValueError("wrong C fixture machine")

                def verify(path, *args):
                    self.assertEqual(path.read_bytes(), b"fresh SDK DLL")
                    return {"preset_documents_abi_version"}

                shared.verify_library.side_effect = verify
                bindings = Mock()
                package_hash = hashlib.sha256(b"package").hexdigest()

                def pack(dotnet, project, *, output, **options):
                    (output / "NaviApps.PresetDocuments.0.1.0.nupkg").write_bytes(b"package")

                bindings.pack_project.side_effect = pack
                bindings.audit_package.return_value = package_hash

                def stage_native(source, destination):
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, destination)
                    if error == "digest" and source == library:
                        destination.write_bytes(b"unverified library")
                    return destination

                bindings.stage_native.side_effect = stage_native
                artifact_tooling = load("artifacts")
                bindings.build_lock.side_effect = artifact_tooling.build_lock
                bindings.hashes.side_effect = artifact_tooling.hashes
                bindings.file_sha256.side_effect = artifact_tooling.file_sha256
                bindings.write_json.side_effect = artifact_tooling.write_json
                bindings.publish.side_effect = artifact_tooling.publish
                tools = {
                    "windows": shared,
                    "c_compiler": bindings,
                    "dotnet": bindings,
                    "nuget": bindings,
                    "artifacts": bindings,
                    "vm_environment": load("vm_environment"),
                }
                support = {"@sdk-tooling/sdk_tooling/windows.py": "b" * 64}
                after = dict.fromkeys(support, "c" * 64) if error == "support" else support
                workspace = ("a" * 40, "a" * 64)
                with (
                    patch.dict(
                        os.environ,
                        {
                            "DOTNET_BIN": str(dotnet),
                            "DOTNET_HOST_PATH": "stale ARM64 path",
                            "VM_RUNNER_ARTIFACTS_DIR": str(artifacts),
                        },
                        clear=True,
                    ),
                    patch.object(driver, "ROOT", root),
                    patch.object(driver, "ffi_product", return_value=native) as builder,
                    patch.object(driver, "load", side_effect=tools.__getitem__),
                    patch.object(driver, "support_inputs", side_effect=(support, after)),
                    patch.object(windows_delivery, "acceptance_source", return_value=workspace),
                    patch.multiple(
                        windows_delivery, windows=shared, artifacts=bindings, c_compiler=bindings
                    ),
                    patch.object(windows_delivery.subprocess, "run") as commands,
                    patch.object(
                        windows_delivery.subprocess, "check_output", return_value="10.0.403"
                    ),
                    patch.object(driver.platform, "platform", return_value="mock Windows"),
                    patch("builtins.print"),
                ):
                    if error:
                        messages = {
                            "receipt": "library differs from the selected host build",
                            "digest": "Staged DLL or import library differs from its verified receipt",
                            "C machine": "wrong C fixture machine",
                            "support": "acceptance support files changed",
                        }
                        with self.assertRaisesRegex(ValueError, messages[error]):
                            driver.main([])
                        self.assertFalse(artifacts.exists())
                        self.assertFalse((root / ".native-build" / rid).exists())
                        if error != "support":
                            bindings.run_managed.assert_not_called()
                            bindings.pack_project.assert_not_called()
                            self.assertFalse(
                                any(
                                    call.args[0][0].endswith(".exe")
                                    for call in commands.call_args_list
                                )
                            )
                        continue
                    driver.main([])

                builder.assert_called_once_with(root)
                shared.native_environment.assert_called_once_with(
                    host,
                    {"DOTNET_BIN": str(dotnet), "VM_RUNNER_ARTIFACTS_DIR": str(artifacts)},
                    target_architecture="x64",
                )
                shared.build_library.assert_called_once_with(
                    native, env, root / "msvc", "x64", native_host=host
                )
                shared.verify_artifact.assert_called_once_with(native, "x64", environment=env)
                self.assertEqual(env["CARGO_TARGET_DIR"], str(redirected))
                self.assertEqual(env["RUSTUP_AUTO_INSTALL"], "0")
                self.assertFalse(
                    any(call.args[0][:2] == ["cargo", "build"] for call in commands.call_args_list)
                )
                for call in commands.call_args_list:
                    args = call.args[0]
                    if "--target" in args:
                        self.assertEqual(args[args.index("--target") + 1], target)
                c = bindings.compile_c.call_args.kwargs
                candidate = c["output"].parent
                verified_library = candidate / library.name
                self.assertEqual(shared.verify_library.call_args.args[0], verified_library)
                self.assertEqual(shared.verify_library.call_args.args[-1], "x64")
                shared.require_pe_machine.assert_called_once_with(c["output"], "x64")
                self.assertEqual(candidate.parent, root / ".native-build")
                self.assertEqual(c["env"]["PATH"], "owned-toolchain")
                staged_import = candidate / import_library.name
                self.assertEqual(
                    bindings.stage_native.call_args_list[0].args, (library, verified_library)
                )
                self.assertEqual(
                    bindings.stage_native.call_args_list[1].args, (import_library, staged_import)
                )
                self.assertEqual(c["libraries"], [staged_import])
                self.assertFalse(candidate.exists())
                self.assertEqual(bindings.stage_native.call_args.args[0], verified_library)
                managed = bindings.run_managed.call_args.kwargs
                self.assertEqual(bindings.run_managed.call_args.args[0], dotnet)
                self.assertEqual(managed["runtime"], rid)
                self.assertEqual(
                    managed["properties"], {"PresetDocumentsNativeLibrary": str(verified_library)}
                )
                self.assertEqual(
                    bindings.pack_project.call_args.kwargs["properties"],
                    {
                        "Version": "0.1.0",
                        "PresetDocumentsRuntimeIdentifier": rid,
                        "PresetDocumentsNativeLibrary": str(
                            candidate / "native" / rid / library.name
                        ),
                    },
                )
                self.assertEqual(
                    bindings.audit_package.call_args.kwargs["runtime_files"],
                    {f"runtimes/{rid}/native/preset_documents.dll": verified_library},
                )
                consumer = bindings.run_isolated_consumer.call_args.kwargs
                self.assertEqual(bindings.run_isolated_consumer.call_args.args[0], dotnet)
                self.assertEqual(consumer["runtime"], rid)
                self.assertTrue(
                    any(verified_library.is_relative_to(path) for path in consumer["exclude_paths"])
                )
                self.assertIn(published, consumer["exclude_paths"])
                self.assertFalse(consumer.get("clear_environment"))
                self.assertEqual(consumer["source"], fixture.read_text(encoding="utf-8"))
                evidence = artifacts / rid / native.version
                receipt = json.loads((evidence / driver.RECEIPT).read_bytes())
                self.assertEqual(receipt["target"], target)
                self.assertEqual(receipt["runtime"], rid)
                execution = {"arm64": "x64-emulated-on-arm64", "x64": "x64-native"}
                self.assertEqual(receipt["execution"], execution[host])
                self.assertEqual(receipt["toolchain"], toolchain)
                self.assertEqual(receipt["toolInputs"], support)
                self.assertEqual(receipt["dotnetVersion"], "10.0.403")
                self.assertEqual((receipt["source"], receipt["workspaceFingerprint"]), workspace)
                package = evidence / "NaviApps.PresetDocuments.0.1.0.nupkg"
                self.assertEqual(receipt["packages"], {package.name: package_hash})
                self.assertEqual(hashlib.sha256(package.read_bytes()).hexdigest(), package_hash)
                local = root / ".native-build" / rid / native.version
                self.assertEqual(
                    (local / driver.RECEIPT).read_bytes(), (evidence / driver.RECEIPT).read_bytes()
                )
                self.assertEqual((local / package.name).read_bytes(), package.read_bytes())
                self.assertEqual(
                    receipt["dllSha256"], hashlib.sha256(library.read_bytes()).hexdigest()
                )
                self.assertEqual(receipt["exports"], ["preset_documents_abi_version"])

    def test_rejects_arm64_target_instead_of_selecting_a_native_package(self):
        with (
            patch("sys.stderr"),
            patch.object(driver, "load") as loader,
            self.assertRaises(SystemExit) as error,
        ):
            driver.main(["--architecture", "arm64"])
        self.assertEqual(error.exception.code, 2)
        loader.assert_not_called()

    def test_wrong_x64_dotnet_sdk_stops_before_cargo_or_native_build(self):
        shared = Mock()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "mise.toml").write_text('[tools]\ndotnet = "10.0.403"\n')
            with (
                patch.object(driver, "ROOT", root),
                patch.object(windows_delivery, "acceptance_source", return_value=("a" * 40, None)),
                patch.object(windows_delivery, "windows", shared),
                patch.object(windows_delivery.subprocess, "check_output", return_value="10.0.402"),
                patch.object(windows_delivery.subprocess, "run") as run,
            ):
                shared.host_architecture.return_value = "x64"
                shared.MSVC_TARGETS = {"x64": windows_delivery.TARGET}
                shared.native_environment.return_value = ({}, root / "MSVC", root / "dotnet.exe")
                with self.assertRaisesRegex(ValueError, "pinned x64 .NET SDK"):
                    driver.main([])
        run.assert_not_called()
        shared.build_library.assert_not_called()


if __name__ == "__main__":
    unittest.main()
