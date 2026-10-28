#!/usr/bin/env python3
"""Artifact-boundary regressions; execution qualification uses wasi_run.py."""
import io
import json
from pathlib import Path
import tarfile
import tempfile
import subprocess
import sys
import unittest

import wasi_release as release

REVISION = "a" * 40
VERSION = "1.2.3"


class BundleTests:
    """Every bundle-boundary regression, run once per release target."""
    TARGET = None

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.runtime = self.root / "runtime.a"
        member = b"\0asm\x01\0\0\0"
        header = f"{'fixture.o/':<16}{0:<12}{0:<6}{0:<6}{'100644':<8}{len(member):<10}`\n".encode()
        self.runtime.write_bytes(b"!<arch>\n" + header + member)
        self.bundle = release.package(self.runtime, self.root / "bundle", REVISION, VERSION, self.TARGET)

    def rewrite(self, transform):
        with tarfile.open(self.bundle) as source:
            entries = [(item, source.extractfile(item).read()) for item in source]
        entries = transform(entries)
        with tarfile.open(self.bundle, "w:gz") as archive:
            for item, contents in entries:
                item.size = len(contents)
                archive.addfile(item, io.BytesIO(contents))

    def test_runtime_is_installed_without_native_archives_or_repository_files(self):
        target = self.root / f"installed/{self.TARGET}/release"
        release.install(self.bundle, target, REVISION, VERSION, self.TARGET)
        self.assertEqual({p.name for p in target.iterdir()}, {release.LIBRARY, "manifest.json"})
        self.assertEqual((target / release.LIBRARY).read_bytes(), self.runtime.read_bytes())
        manifest = json.loads((target / "manifest.json").read_text())
        self.assertEqual(manifest["source_revision"], REVISION)

    def test_bundle_is_deterministic(self):
        second = release.package(self.runtime, self.root / "second", REVISION, VERSION, self.TARGET)
        self.assertEqual(self.bundle.read_bytes(), second.read_bytes())

    def test_source_and_version_must_both_match(self):
        for revision, version in [("b" * 40, VERSION), (REVISION, "1.2.4")]:
            with self.assertRaises(ValueError):
                release.verify(self.bundle, revision, version, self.TARGET)

    def test_corrupt_payload_cannot_replace_an_installed_runtime(self):
        def corrupt(entries):
            return [(item, contents[:-1] + b"x" if item.name == release.LIBRARY else contents)
                    for item, contents in entries]
        self.rewrite(corrupt)
        target = self.root / "installed"
        target.mkdir()
        (target / release.LIBRARY).write_bytes(b"previous runtime")
        with self.assertRaisesRegex(ValueError, "checksum"):
            release.install(self.bundle, target, REVISION, VERSION, self.TARGET)
        self.assertEqual((target / release.LIBRARY).read_bytes(), b"previous runtime")

    def test_unexpected_paths_symlinks_and_duplicate_members_are_rejected(self):
        original = self.bundle.read_bytes()
        for variant in ["path", "symlink", "duplicate"]:
            self.bundle.write_bytes(original)
            def change(entries):
                if variant == "duplicate":
                    return entries + [entries[0]]
                item, contents = entries[0]
                if variant == "path":
                    item.name = "../manifest.json"
                else:
                    item.type, item.linkname = tarfile.SYMTYPE, "outside"
                return [(item, contents)] + entries[1:]
            self.rewrite(change)
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                release.verify(self.bundle, REVISION, VERSION, self.TARGET)
        self.assertFalse((self.root / "manifest.json").exists())

    def test_wrong_target_is_rejected_even_when_the_payload_hash_matches(self):
        def change(entries):
            result = []
            for item, contents in entries:
                if item.name == "manifest.json":
                    manifest = json.loads(contents)
                    manifest["target_triple"] = "x86_64-unknown-linux-gnu"
                    contents = json.dumps(manifest).encode()
                result.append((item, contents))
            return result
        self.rewrite(change)
        with self.assertRaises(ValueError):
            release.verify(self.bundle, REVISION, VERSION, self.TARGET)

    def test_missing_runtime_and_invalid_archive_are_not_published(self):
        self.runtime.write_bytes(b"not a runtime archive")
        output = self.root / "bad"
        with self.assertRaises(ValueError):
            release.package(self.runtime, output, REVISION, VERSION, self.TARGET)
        self.assertFalse((output / release.bundle_name(self.TARGET)).exists())

    def test_cli_cannot_stamp_a_different_version_onto_the_release_checkout(self):
        output = self.root / "wrong-version"
        result = subprocess.run([sys.executable, str(release.ROOT / "scripts/wasi_release.py"),
            "package", "--target", self.TARGET, "--runtime", str(self.runtime),
            "--output", str(output), "--revision", REVISION, "--version", "0.0.0-invalid"], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("package version must match", result.stderr)
        self.assertFalse(output.exists())

    def test_a_bundle_only_verifies_for_its_own_target(self):
        other = next(t for t in release.TARGETS if t != self.TARGET)
        self.assertEqual(self.bundle.name, f"perry-cross-{self.TARGET}.tar.gz")
        self.assertEqual(json.loads(self.manifest())["target_triple"], self.TARGET)
        with self.assertRaisesRegex(ValueError, "target"):
            release.verify(self.bundle, REVISION, VERSION, other)

    def manifest(self):
        with tarfile.open(self.bundle) as archive:
            return archive.extractfile("manifest.json").read()


class Wasip2BundleTests(BundleTests, unittest.TestCase):
    TARGET = "wasm32-wasip2"

    def test_the_default_target_is_wasip2(self):
        # Callers that name no target keep today's bundle byte for byte.
        default = release.package(self.runtime, self.root / "default", REVISION, VERSION)
        self.assertEqual(default.read_bytes(), self.bundle.read_bytes())


class Wasip1BundleTests(BundleTests, unittest.TestCase):
    TARGET = "wasm32-wasip1"


if __name__ == "__main__":
    unittest.main()
