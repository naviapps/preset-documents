"""SDK acceptance consumes the native builder's output, including redirected Cargo builds."""

import hashlib
import importlib.util
import json
import os
import shutil
import sys
import tempfile
import unittest
from contextlib import nullcontext
from itertools import product
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from tools._tooling import load


def load_driver():
    tools = Path(__file__).resolve().parents[1]
    spec = importlib.util.spec_from_file_location("preset_documents_windows", tools / "windows.py")
    driver = importlib.util.module_from_spec(spec)
    binding_spec = importlib.util.spec_from_file_location("bindings", tools / "bindings.py")
    binding_driver = importlib.util.module_from_spec(binding_spec)
    with patch.dict(
        sys.modules,
        {
            "_tooling": SimpleNamespace(load=Mock(), support_inputs=Mock()),
            "bindings": binding_driver,
            "native": SimpleNamespace(builder=Mock()),
        },
    ):
        binding_spec.loader.exec_module(binding_driver)
        spec.loader.exec_module(driver)
    return driver


class WindowsOutputTests(unittest.TestCase):
    def test_redirected_x64_build_cannot_consume_missing_or_stale_legacy_output(self):
        driver = load_driver()
        target = "x86_64-pc-windows-msvc"
        rid = "win-x64"
        hosts = {"arm64": "aarch64-pc-windows-msvc", "x64": target}
        for host, error, stale in product(
            hosts, (None, "receipt", "C machine", "support"), (False, True)
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
                fixture = root / "fixtures/dotnet-consumer/Program.cs"
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
                    "NAVI_CI_ARTIFACTS": str(artifacts),
                }
                dotnet = root / "dotnet-x64/dotnet.exe"
                native = Mock(version="0.1.0")
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
                    {"toolchain": {} if error == "receipt" else toolchain},
                )
                if error == "C machine":
                    shared.require_pe_machine.side_effect = ValueError("wrong C fixture machine")

                def verify(path, *args):
                    self.assertEqual(path.read_bytes(), b"fresh SDK DLL")
                    return {"preset_documents_abi_version"}

                shared.verify_library.side_effect = verify
                bindings = Mock()
                artifact_tooling = load("artifacts")
                package_hash = hashlib.sha256(b"package").hexdigest()

                def pack(dotnet, project, *, output, **options):
                    (output / "NaviApps.PresetDocuments.0.1.0.nupkg").write_bytes(b"package")

                bindings.pack_project.side_effect = pack
                bindings.audit_package.return_value = package_hash

                def stage_native(source, destination):
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, destination)
                    return destination

                bindings.stage_native.side_effect = stage_native
                tools = {
                    "windows": shared,
                    "bindings": bindings,
                    "artifacts": artifact_tooling,
                }
                support = {"tools/vendor/sdk_tooling/windows.py": "b" * 64}
                after = dict.fromkeys(support, "c" * 64) if error == "support" else support
                workspace = ("a" * 40, "a" * 64)
                with (
                    patch.dict(
                        os.environ,
                        {"DOTNET_BIN": str(dotnet), "DOTNET_HOST_PATH": "stale ARM64 path"},
                        clear=True,
                    ),
                    patch.object(driver, "ROOT", root),
                    patch.object(driver, "builder", return_value=native) as builder,
                    patch.object(driver, "load", side_effect=tools.__getitem__),
                    patch.object(driver, "support_inputs", side_effect=(support, after)),
                    patch.object(driver, "acceptance_source", return_value=workspace),
                    patch.object(driver.subprocess, "run") as commands,
                    patch.object(driver.subprocess, "check_output", return_value="10.0.403"),
                    patch.object(driver.platform, "platform", return_value="mock Windows"),
                    patch("builtins.print"),
                ):
                    if error:
                        messages = {
                            "receipt": "library differs from the selected host build",
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

                builder.assert_called_once_with()
                shared.native_environment.assert_called_once_with(
                    host, {"DOTNET_BIN": str(dotnet)}, target_architecture="x64"
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
                self.assertEqual(
                    managed["env"]["PRESET_DOCUMENTS_NATIVE_LIBRARY"], str(verified_library)
                )
                self.assertEqual(managed["runtime"], rid)
                self.assertEqual(managed["properties"], {"UseAppHost": "false"})
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
                self.assertEqual(
                    consumer["clear_environment"], ("PRESET_DOCUMENTS_NATIVE_LIBRARY",)
                )
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
        driver = load_driver()
        with (
            patch("sys.stderr"),
            patch.object(driver, "load") as loader,
            self.assertRaises(SystemExit) as error,
        ):
            driver.main(["--architecture", "arm64"])
        self.assertEqual(error.exception.code, 2)
        loader.assert_not_called()

    def test_emulated_host_acceptance_requires_exact_captured_source(self):
        driver = load_driver()
        commit, digest = "a" * 40, "b" * 64
        with patch.dict(
            os.environ,
            {"NAVI_CI_SOURCE_COMMIT": commit, "NAVI_CI_SOURCE_FINGERPRINT": digest},
            clear=True,
        ):
            self.assertEqual(driver.acceptance_source(), (commit, digest))
            del os.environ["NAVI_CI_SOURCE_FINGERPRINT"]
            with self.assertRaisesRegex(ValueError, "workspace SHA-256"):
                driver.acceptance_source()
        with (
            patch.dict(os.environ, {}, clear=True),
            patch.object(driver.subprocess, "check_output", return_value=" M Cargo.lock"),
            self.assertRaisesRegex(ValueError, "clean committed checkout"),
        ):
            driver.acceptance_source()

    def test_wrong_x64_dotnet_sdk_stops_before_cargo_or_native_build(self):
        driver = load_driver()
        shared = Mock()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "mise.toml").write_text('[tools]\ndotnet = "10.0.403"\n')
            with (
                patch.object(driver, "ROOT", root),
                patch.object(driver, "acceptance_source", return_value=("a" * 40, None)),
                patch.object(driver, "load", return_value=shared),
                patch.object(driver.subprocess, "check_output", return_value="10.0.402"),
                patch.object(driver.subprocess, "run") as run,
            ):
                shared.native_environment.return_value = ({}, root / "MSVC", root / "dotnet.exe")
                with self.assertRaisesRegex(ValueError, "pinned x64 .NET SDK"):
                    driver.main(["--architecture", "x64"])
        run.assert_not_called()
        shared.build_library.assert_not_called()

    def test_x64_evidence_checks_destinations_before_replacing_previous_package(self):
        driver = load_driver()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "NaviApps.PresetDocuments.0.1.0.nupkg"
            package.write_bytes(b"new package")
            receipt = {
                "version": "0.1.0",
                "packages": {package.name: hashlib.sha256(package.read_bytes()).hexdigest()},
            }
            output = root / "output"
            external = root / "external"
            local = output / "win-x64" / "0.1.0"
            local.mkdir(parents=True)
            (local / package.name).write_bytes(b"previous package")
            occupied = external / "win-x64" / "0.1.0" / "user-file.txt"
            occupied.parent.mkdir(parents=True)
            occupied.write_text("owner bytes", encoding="utf-8")
            tooling = Mock(build_lock=nullcontext)

            with self.assertRaisesRegex(ValueError, "contains unrelated files"):
                driver.publish_x64(
                    package, receipt, output_dir=output, artifact_root=external, tooling=tooling
                )

            tooling.publish.assert_not_called()
            self.assertEqual((local / package.name).read_bytes(), b"previous package")
            self.assertEqual(occupied.read_text(encoding="utf-8"), "owner bytes")

    def test_x64_evidence_keeps_previous_outputs_when_external_publication_fails(self):
        driver = load_driver()
        tooling = load("artifacts")
        for failure, had_local in product(("stage", "rename", "verify"), (False, True)):
            with (
                self.subTest(failure=failure, had_local=had_local),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary)
                package, receipt, output, artifact_root, local, external = x64_evidence(root)
                if had_local:
                    write_previous_evidence(local, package.name)
                write_previous_evidence(external, package.name)
                replace = os.replace

                def fail_external_replace(source, destination):
                    destination = Path(destination)
                    writes_external_receipt = (
                        destination.name == driver.RECEIPT
                        and destination.is_relative_to(external.parent)
                    )
                    installs_external = (
                        destination == external and Path(source).name == external.name
                    )
                    if (failure == "stage" and writes_external_receipt) or (
                        failure == "rename" and installs_external
                    ):
                        raise PermissionError("external evidence is locked")
                    replace(source, destination)
                    if failure == "verify" and installs_external:
                        (destination / package.name).write_bytes(b"changed during replacement")

                error_type, message = (
                    (ValueError, "differs from the accepted package and receipt")
                    if failure == "verify"
                    else (PermissionError, "external evidence is locked")
                )

                with (
                    patch.object(tooling.os, "replace", side_effect=fail_external_replace),
                    self.assertRaisesRegex(error_type, message),
                ):
                    driver.publish_x64(
                        package,
                        receipt,
                        output_dir=output,
                        artifact_root=artifact_root,
                        tooling=tooling,
                    )

                if had_local:
                    assert_previous_evidence(self, local, package.name)
                else:
                    self.assertFalse(local.exists())
                assert_previous_evidence(self, external, package.name)

    def test_x64_evidence_preserves_an_external_file_that_appears_during_commit(self):
        driver = load_driver()
        tooling = load("artifacts")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package, receipt, output, artifact_root, local, external = x64_evidence(root)
            write_previous_evidence(local, package.name)
            occupied = external / "user-file.txt"
            publish = tooling.publish

            def occupy_after_local_replacement(stage, destination, verify):
                def occupy_then_verify():
                    occupied.parent.mkdir(parents=True, exist_ok=True)
                    occupied.write_bytes(b"owner bytes")
                    verify()

                publish(stage, destination, occupy_then_verify if destination == local else verify)

            with (
                patch.object(tooling, "publish", side_effect=occupy_after_local_replacement),
                self.assertRaisesRegex(ValueError, "contains unrelated files"),
            ):
                driver.publish_x64(
                    package,
                    receipt,
                    output_dir=output,
                    artifact_root=artifact_root,
                    tooling=tooling,
                )

            self.assertEqual({path.name for path in external.iterdir()}, {occupied.name})
            self.assertEqual(occupied.read_bytes(), b"owner bytes")
            assert_previous_evidence(self, local, package.name)

    def test_x64_evidence_keeps_both_verified_outputs_if_external_cleanup_fails(self):
        driver = load_driver()
        tooling = load("artifacts")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package, receipt, output, artifact_root, local, external = x64_evidence(root)
            write_previous_evidence(local, package.name)
            write_previous_evidence(external, package.name)
            rmtree = shutil.rmtree

            def fail_external_cleanup(path, *args, **kwargs):
                path = Path(path)
                if path.parent == external.parent and path.name.startswith(".0.1.0.rollback-"):
                    raise PermissionError("previous external evidence is locked")
                return rmtree(path, *args, **kwargs)

            with (
                patch.object(tooling.shutil, "rmtree", side_effect=fail_external_cleanup),
                self.assertRaisesRegex(PermissionError, "previous external evidence is locked"),
            ):
                driver.publish_x64(
                    package,
                    receipt,
                    output_dir=output,
                    artifact_root=artifact_root,
                    tooling=tooling,
                )

            for destination in (local, external):
                self.assertEqual((destination / package.name).read_bytes(), b"new package")
                self.assertEqual(json.loads((destination / driver.RECEIPT).read_bytes()), receipt)
            (recovery,) = external.parent.glob(".0.1.0.rollback-*")
            assert_previous_evidence(self, recovery / "previous-artifacts", package.name)


def x64_evidence(root):
    package = root / "NaviApps.PresetDocuments.0.1.0.nupkg"
    package.write_bytes(b"new package")
    receipt = {
        "version": "0.1.0",
        "packages": {package.name: hashlib.sha256(package.read_bytes()).hexdigest()},
    }
    output = root / "output"
    artifacts = root / "external"
    relative = Path("win-x64") / receipt["version"]
    return package, receipt, output, artifacts, output / relative, artifacts / relative


def write_previous_evidence(directory, package_name):
    directory.mkdir(parents=True)
    (directory / package_name).write_bytes(b"previous package")
    (directory / "preset-documents-windows.json").write_bytes(b"previous receipt")


def assert_previous_evidence(test, directory, package_name):
    test.assertEqual((directory / package_name).read_bytes(), b"previous package")
    test.assertEqual(
        (directory / "preset-documents-windows.json").read_bytes(), b"previous receipt"
    )


if __name__ == "__main__":
    unittest.main()
