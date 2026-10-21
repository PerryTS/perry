#!/usr/bin/env python3
"""Artifact-boundary regressions; execution qualification uses wasi_run.py."""
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

import wasi_release as release

REVISION = "a" * 40
VERSION = "1.2.3"


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.runtime = self.root / "runtime.a"
        member = b"\0asm\x01\0\0\0"
        header = f"{'fixture.o/':<16}{0:<12}{0:<6}{0:<6}{'100644':<8}{len(member):<10}`\n".encode()
        self.runtime.write_bytes(b"!<arch>\n" + header + member)
        self.bundle = release.package(self.runtime, self.root / "bundle", REVISION, VERSION)

    def rewrite(self, transform):
        with tarfile.open(self.bundle) as source:
            entries = [(item, source.extractfile(item).read()) for item in source]
        entries = transform(entries)
        with tarfile.open(self.bundle, "w:gz") as archive:
            for item, contents in entries:
                item.size = len(contents)
                archive.addfile(item, io.BytesIO(contents))

    def test_runtime_is_installed_without_native_archives_or_repository_files(self):
        target = self.root / "installed/wasm32-wasip2/release"
        release.install(self.bundle, target, REVISION, VERSION)
        self.assertEqual({p.name for p in target.iterdir()}, {release.LIBRARY, "manifest.json"})
        self.assertEqual((target / release.LIBRARY).read_bytes(), self.runtime.read_bytes())
        manifest = json.loads((target / "manifest.json").read_text())
        self.assertEqual(manifest["source_revision"], REVISION)

    def test_bundle_is_deterministic(self):
        second = release.package(self.runtime, self.root / "second", REVISION, VERSION)
        self.assertEqual(self.bundle.read_bytes(), second.read_bytes())

    def test_source_and_version_must_both_match(self):
        for revision, version in [("b" * 40, VERSION), (REVISION, "1.2.4")]:
            with self.assertRaises(ValueError):
                release.verify(self.bundle, revision, version)

    def test_corrupt_payload_cannot_replace_an_installed_runtime(self):
        def corrupt(entries):
            return [(item, contents[:-1] + b"x" if item.name == release.LIBRARY else contents)
                    for item, contents in entries]
        self.rewrite(corrupt)
        target = self.root / "installed"
        target.mkdir()
        (target / release.LIBRARY).write_bytes(b"previous runtime")
        with self.assertRaisesRegex(ValueError, "checksum"):
            release.install(self.bundle, target, REVISION, VERSION)
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
                release.verify(self.bundle, REVISION, VERSION)
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
            release.verify(self.bundle, REVISION, VERSION)

    def test_missing_runtime_and_invalid_archive_are_not_published(self):
        self.runtime.write_bytes(b"not a runtime archive")
        output = self.root / "bad"
        with self.assertRaises(ValueError):
            release.package(self.runtime, output, REVISION, VERSION)
        self.assertFalse((output / release.BUNDLE).exists())


if __name__ == "__main__":
    unittest.main()
