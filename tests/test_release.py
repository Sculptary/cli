"""Release packaging regressions. Uses only the Python standard library."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release_tools", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="sculpt-release-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        self.output = self.root / "output"
        self.tag = "v1.2.3"
        for target, binary in release.TARGETS.items():
            path = self.root / binary
            path.write_bytes(f"Fixture for {target}\n".encode())
            release.archive(self.tag, target, path, self.artifacts)

    def generate(self):
        release.generate(self.tag, self.artifacts, self.output)

    def test_hashes_match_actual_archives_and_generated_files(self):
        self.generate()
        manifest = dict(line.split("  ", 1)[::-1] for line in (self.output / "SHA256SUMS").read_text().splitlines())
        self.assertEqual(set(manifest), {p.name for p in self.output.iterdir()} - {"SHA256SUMS"})
        for name, digest in manifest.items():
            self.assertEqual(digest, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        scoop = json.loads((self.output / "sculpt.json").read_text())
        name = release.asset_name(self.tag, "x86_64-pc-windows-msvc")
        self.assertEqual(scoop["version"], "1.2.3")
        self.assertEqual(scoop["architecture"]["64bit"]["hash"], manifest[name])
        self.assertEqual(scoop["bin"], "sculpt.exe")
        for target in release.TARGETS:
            asset = release.asset_name(self.tag, target)
            if "windows" not in target:
                self.assertIn(manifest[asset], (self.output / "sculpt.rb").read_text())
            self.assertIn(manifest[asset], (self.output / ("install.ps1" if "windows" in target else "install.sh")).read_text())
        release.verify(self.output)

    def test_archives_are_reproducible_and_include_licences(self):
        target = "x86_64-unknown-linux-gnu"
        binary = self.root / "binary"
        binary.write_bytes(b"immutable fixture")
        first = release.archive(self.tag, target, binary, self.root / "first")
        second = release.archive(self.tag, target, binary, self.root / "second")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as packed:
            self.assertEqual(set(packed.getnames()), {"sculpt", "LICENSE-MIT", "LICENSE-APACHE"})
            self.assertEqual(packed.getmember("sculpt").mode, 0o755)
            self.assertEqual(packed.extractfile("sculpt").read(), b"immutable fixture")

    def test_generation_is_idempotent_in_a_merged_release_directory(self):
        release.generate(self.tag, self.artifacts, self.artifacts)
        first = {p.name: p.read_bytes() for p in self.artifacts.iterdir()}
        release.generate(self.tag, self.artifacts, self.artifacts)
        self.assertEqual(first, {p.name: p.read_bytes() for p in self.artifacts.iterdir()})

    def test_missing_or_mixed_release_artifacts_fail_before_writing(self):
        (self.artifacts / release.asset_name(self.tag, next(iter(release.TARGETS)))).unlink()
        with self.assertRaises(ValueError): self.generate()
        self.assertFalse(self.output.exists())
        (self.artifacts / "sculpt-v9.0.0-other.tar.gz").write_bytes(b"wrong release")
        with self.assertRaises(ValueError): self.generate()

    def test_corrupt_archives_and_archive_links_are_rejected(self):
        name = release.asset_name(self.tag, "x86_64-unknown-linux-gnu")
        path = self.artifacts / name
        path.write_bytes(b"not a tarball")
        with self.assertRaises(ValueError): self.generate()
        for entry_name, kind in [("sculpt", tarfile.SYMTYPE), ("../sculpt", tarfile.REGTYPE), ("sculpt", tarfile.LNKTYPE)]:
            with self.subTest(entry_name=entry_name, kind=kind):
                with tarfile.open(path, "w:gz") as packed:
                    entry = tarfile.TarInfo(entry_name)
                    entry.type = kind
                    entry.linkname = "outside"
                    packed.addfile(entry, io.BytesIO())
                with self.assertRaises(ValueError): self.generate()

    def test_verification_detects_tampering_and_manifest_path_traversal(self):
        self.generate()
        (self.output / "install.sh").write_text("tampered")
        with self.assertRaises(ValueError): release.verify(self.output)
        (self.output / "SHA256SUMS").write_text("0" * 64 + "  ../outside\n")
        with self.assertRaises(ValueError): release.verify(self.output)

    def test_tags_are_validated_before_use_in_paths_or_templates(self):
        for bad in ["1.2.3", "v01.2.3", "v1.2", "v1.2.3/evil", "v1.2.3'", "v1.2.3\n", "v1.2.3-01", "v1.2.3+", "../../evil"]:
            with self.subTest(tag=bad):
                with self.assertRaises(ValueError): release.version(bad)
        for good in ["v0.5.0", "v1.2.3-rc.1", "v1.2.3+build.2"]:
            self.assertEqual(release.version(good), good[1:])
        with self.assertRaises(ValueError): release.validate_tag("v1.2.3", "1.2.4")


if __name__ == "__main__":
    unittest.main()
