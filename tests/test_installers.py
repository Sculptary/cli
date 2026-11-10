"""Execute generated installers with local archive fixtures and real extraction."""

import os
import platform
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_release

release = test_release.release


class InstallerFixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="sculpt-installer-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "artifacts"
        self.output = self.root / "release"
        self.destination = self.root / "install [fixture] directory"
        self.scratch = self.root / "scratch"
        self.scratch.mkdir()
        self.tag = "v1.2.3"
        for target, binary in release.TARGETS.items():
            source = self.root / binary
            source.write_bytes(b"#!/bin/sh\nprintf 'sculpt fixture\\n'\n")
            release.archive(self.tag, target, source, self.artifacts)
        release.generate(self.tag, self.artifacts, self.output)

    def assert_clean(self):
        self.assertEqual(list(self.scratch.iterdir()), [])
        if self.destination.exists():
            self.assertFalse(list(self.destination.glob(".sculpt*")))


@unittest.skipUnless(os.name == "posix", "POSIX shell runs on the Linux and macOS jobs")
class PosixInstallerTests(InstallerFixture):
    def setUp(self):
        super().setUp()
        self.tools = self.root / "tools"
        self.tools.mkdir()
        for name, text in {
            "uname": '#!/bin/sh\nif [ "$1" = -s ]; then printf "%s\\n" "$TEST_SYSTEM"; else printf "%s\\n" "$TEST_MACHINE"; fi\n',
            "curl": '#!/bin/sh\n[ "${TEST_DOWNLOAD_FAIL:-0}" != 1 ] || exit 22\nprintf "%s\\n" "$*" > "$TEST_DOWNLOAD_LOG"\nwhile [ "$#" -gt 0 ]; do if [ "$1" = --output ]; then cp "$TEST_ARCHIVE" "$2"; exit $?; fi; shift; done\nexit 2\n',
        }.items():
            path = self.tools / name
            path.write_text(text)
            path.chmod(0o755)

    def run_installer(
        self,
        system="Linux",
        machine="x86_64",
        target="x86_64-unknown-linux-gnu",
        **overrides,
    ):
        env = {
            **os.environ,
            "PATH": str(self.tools) + os.pathsep + os.environ["PATH"],
            "SCULPT_INSTALL_DIR": str(self.destination),
            "TEST_ARCHIVE": str(self.artifacts / release.asset_name(self.tag, target)),
            "TEST_DOWNLOAD_LOG": str(self.root / "download.log"),
            "TEST_SYSTEM": system,
            "TEST_MACHINE": machine,
            "TMPDIR": str(self.scratch),
            **overrides,
        }
        return subprocess.run(
            ["sh", str(self.output / "install.sh")],
            env=env,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )

    def test_installs_all_supported_unix_targets_and_replaces_an_existing_binary(self):
        for system, machine, target in [
            ("Linux", "x86_64", "x86_64-unknown-linux-gnu"),
            ("Darwin", "arm64", "aarch64-apple-darwin"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
        ]:
            with self.subTest(target=target):
                self.destination.mkdir(exist_ok=True)
                (self.destination / "sculpt").write_text("old binary")
                result = self.run_installer(system, machine, target)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(
                    subprocess.check_output(
                        [str(self.destination / "sculpt")], text=True
                    ),
                    "sculpt fixture\n",
                )
                download = (self.root / "download.log").read_text()
                self.assertIn("--proto =https --proto-redir =https", download)
                self.assertIn(release.asset_name(self.tag, target), download)
                self.assert_clean()

    def test_checksum_and_download_failures_preserve_existing_binary(self):
        self.destination.mkdir()
        existing = self.destination / "sculpt"
        existing.write_text("old binary")
        archive = self.artifacts / release.asset_name(
            self.tag, "x86_64-unknown-linux-gnu"
        )
        archive.write_bytes(b"tampered")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum", result.stderr)
        self.assertEqual(existing.read_text(), "old binary")
        self.assert_clean()
        result = self.run_installer(TEST_DOWNLOAD_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(existing.read_text(), "old binary")
        self.assert_clean()

    def test_unsupported_target_stops_before_download(self):
        result = self.run_installer(machine="aarch64")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "download.log").exists())
        self.assertFalse(self.destination.exists())
        self.assert_clean()

    @unittest.skipUnless(
        os.environ.get("SCULPT_TEST_BINARY"),
        "Set SCULPT_TEST_BINARY for compiled CLI smoke coverage",
    )
    def test_installs_and_runs_the_compiled_cli(self):
        system = platform.system()
        machine = platform.machine()
        target = (
            "aarch64-apple-darwin"
            if system == "Darwin" and machine in {"arm64", "aarch64"}
            else "x86_64-apple-darwin"
            if system == "Darwin"
            else "x86_64-unknown-linux-gnu"
        )
        release.archive(
            self.tag, target, Path(os.environ["SCULPT_TEST_BINARY"]), self.artifacts
        )
        release.generate(self.tag, self.artifacts, self.output)
        result = self.run_installer(system, machine, target)
        self.assertEqual(result.returncode, 0, result.stderr)
        output = subprocess.check_output(
            [str(self.destination / "sculpt"), "--version"], text=True
        )
        self.assertRegex(output, r"^sculpt \d+\.\d+\.\d+")
        self.assert_clean()


@unittest.skipUnless(os.name == "nt", "PowerShell installer runs on Windows")
class WindowsInstallerTests(InstallerFixture):
    def run_installer(self, fail=False, unsupported=False, shadow_tar=False):
        quoted = lambda path: str(path).replace("'", "''")
        wrapper = self.root / "test.ps1"
        source = self.artifacts / release.asset_name(self.tag, "x86_64-pc-windows-msvc")
        wrapper.write_text(
            f"""
$ErrorActionPreference = 'Stop'
function Invoke-WebRequest {{
    param([switch]$UseBasicParsing,[string]$Uri,[string]$OutFile,[int]$TimeoutSec)
    if ($Uri -ne '{release.DOWNLOADS}/{self.tag}/{source.name}') {{ throw 'Unexpected download URL' }}
    {'throw "Fixture download failure"' if fail else f"Copy-Item -LiteralPath '{quoted(source)}' -Destination $OutFile"}
}}
try {{ & '{quoted(self.output / "install.ps1")}' -InstallDir '{quoted(self.destination)}'; exit 0 }}
catch {{ Write-Output $_.Exception.Message; exit 1 }}
""",
            encoding="utf-8",
        )
        env = {**os.environ, "TEMP": str(self.scratch), "TMP": str(self.scratch)}
        if shadow_tar:
            tools = self.root / "tools"
            tools.mkdir()
            (tools / "tar.cmd").write_text(
                "@echo off\necho Wrong tar selected >&2\nexit /b 125\n"
            )
            env["PATH"] = str(tools) + os.pathsep + env["PATH"]
        if unsupported:
            env.update(PROCESSOR_ARCHITECTURE="ARM64", PROCESSOR_ARCHITEW6432="")
        return subprocess.run(
            [
                "powershell",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(wrapper),
            ],
            env=env,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )

    def test_installs_archive_into_a_path_with_spaces_and_literal_brackets(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.destination / "sculpt.exe").is_file())
        self.assert_clean()

    def test_uses_native_windows_tar_when_another_tar_is_first_on_path(self):
        result = self.run_installer(shadow_tar=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.destination / "sculpt.exe").is_file())
        self.assert_clean()

    def test_failed_checksum_and_download_leave_existing_binary_intact(self):
        self.destination.mkdir()
        existing = self.destination / "sculpt.exe"
        existing.write_bytes(b"old binary")
        (
            self.artifacts / release.asset_name(self.tag, "x86_64-pc-windows-msvc")
        ).write_bytes(b"tampered")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum", result.stdout)
        self.assertEqual(existing.read_bytes(), b"old binary")
        self.assert_clean()
        result = self.run_installer(fail=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(existing.read_bytes(), b"old binary")
        self.assert_clean()

    def test_unsupported_architecture_does_not_create_destination(self):
        result = self.run_installer(unsupported=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.destination.exists())
        self.assert_clean()

    @unittest.skipUnless(
        os.environ.get("SCULPT_TEST_BINARY"),
        "Set SCULPT_TEST_BINARY for compiled CLI smoke coverage",
    )
    def test_installs_and_runs_the_compiled_cli(self):
        release.archive(
            self.tag,
            "x86_64-pc-windows-msvc",
            Path(os.environ["SCULPT_TEST_BINARY"]),
            self.artifacts,
        )
        release.generate(self.tag, self.artifacts, self.output)
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        output = subprocess.check_output(
            [str(self.destination / "sculpt.exe"), "--version"], text=True
        )
        self.assertRegex(output, r"^sculpt \d+\.\d+\.\d+")
        self.assert_clean()


if __name__ == "__main__":
    unittest.main()
