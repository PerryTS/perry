"""Release asset regressions: exact npm bytes, complete sets and archive layout."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("select_assets", ROOT / "scripts/publish/select-target-assets.py")
assets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(assets)
VERSION = "0.5.1665"
NAME = "@perryts/perry-target-wasm32-wasip2"


class ReleaseAssetTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.catalog = self.root / "catalog.json"
        self.catalog.write_text(json.dumps({"targets": ["wasm32-wasip2"], "ext": []}))
        self.packages = self.root / "packages"

    def pack(self, attempt, version=VERSION, payload=b"runtime"):
        path = self.packages / attempt / f"perryts-perry-target-wasm32-wasip2-{version}.tgz"
        path.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(path, "w:gz") as archive:
            for name, content in {
                "package/package.json": json.dumps({"name": NAME, "version": version}).encode(),
                "package/target.json": json.dumps({"target_triple": "wasm32-wasip2", "perry_version": version}).encode(),
                "package/runtime.a": payload,
            }.items():
                member = tarfile.TarInfo(name)
                member.size = len(content)
                archive.addfile(member, io.BytesIO(content))
        return path

    def test_registry_selects_exact_bytes_across_attempts(self):
        self.pack("attempt-1", payload=b"unpublished rebuild")
        published = self.pack("attempt-2")
        sha = hashlib.sha1(published.read_bytes()).hexdigest()
        with patch.object(assets.subprocess, "check_output", return_value=json.dumps(sha)) as npm:
            rows = assets.select(self.packages, self.catalog, VERSION, True)
        self.assertEqual(rows[0][0], str(published.resolve()))
        self.assertIn(f"{NAME}@{VERSION}", npm.call_args.args[0])

    def test_modified_bytes_are_refused(self):
        packed = self.pack("attempt-1")
        sha = hashlib.sha1(packed.read_bytes()).hexdigest()
        with packed.open("ab") as stream:
            stream.write(b"tampered")
        with patch.object(assets.subprocess, "check_output", return_value=json.dumps(sha)):
            with self.assertRaisesRegex(ValueError, "no artifact matches public npm SHA-1"):
                assets.select(self.packages, self.catalog, VERSION, True)

    def test_missing_package_and_wrong_version_are_refused(self):
        with self.assertRaisesRegex(ValueError, "missing packed package"):
            assets.select(self.packages, self.catalog, VERSION, False)
        self.pack("attempt-1", version="0.5.1664")
        with self.assertRaisesRegex(ValueError, "version does not match"):
            assets.select(self.packages, self.catalog, VERSION, False)

    def test_signing_extracts_the_archive_writers_member(self):
        self.pack("attempt-1")
        # A fixture signer avoids secrets; this tests the real shell boundary.
        signer = self.root / "perry"
        signer.write_text('#!/bin/sh\nprintf \'{"signed":true}\\n\'\n')
        signer.chmod(0o755)
        archive = self.root / "host.tar.gz"
        subprocess.run(["tar", "czf", str(archive), "perry"], cwd=self.root, check=True)
        out = self.root / "out"
        env = dict(os.environ, PACKAGED_TRIPLES=str(self.catalog), REPO="PerryTS/perry", TAG=f"v{VERSION}",
                   PERRY_CLI_UPDATE_SIGNING_KEY="throwaway", PERRY_CLI_UPDATE_KEY_ID="test",
                   RUNNER_TEMP=str(self.root), VERIFY_NPM="false", RELEASE_VERSION=VERSION)
        result = subprocess.run(["bash", "scripts/publish/sign-target-assets.sh", str(archive),
                                 str(self.packages), str(out)], cwd=ROOT, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = out / f"perry-target-wasm32-wasip2-{VERSION}.tgz.update.json"
        self.assertEqual(json.loads(manifest.read_text()), {"signed": True})


if __name__ == "__main__":
    unittest.main()
