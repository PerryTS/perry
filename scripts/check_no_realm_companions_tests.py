#!/usr/bin/env python3
from pathlib import Path
import json
import tempfile
import unittest
from check_no_realm_companions import ALLOWED, companions, violations


def write(path, count):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"version": 1, "groups": {"objects": [{}] * count}}))


class CompanionRatchetTests(unittest.TestCase):
    def test_no_companion_is_allowed(self):
        self.assertEqual(ALLOWED, {})

    def test_nested_source_companion_is_rejected_and_build_output_is_allowed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "crates/example/src/nested"
            source.mkdir(parents=True)
            output = root / "target/debug/build/runtime/out"
            output.mkdir(parents=True)
            (output / "module.realm.json").write_text("{}")
            self.assertEqual(violations(root, {}), [])
            write(source / "body.realm.json", 1)
            self.assertEqual(companions(root), [Path("crates/example/src/nested/body.realm.json")])
            self.assertEqual(len(violations(root, {})), 1)

    def test_listed_file_may_shrink_or_vanish_but_not_grow(self):
        allowed = {"crates/a/src/x.realm.json": 3}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.assertEqual(violations(root / ".", allowed) if (root / "crates").exists() else [], [])
            (root / "crates").mkdir()
            self.assertEqual(violations(root, allowed), [])
            write(root / "crates/a/src/x.realm.json", 3)
            self.assertEqual(violations(root, allowed), [])
            write(root / "crates/a/src/x.realm.json", 2)
            self.assertEqual(violations(root, allowed), [])
            write(root / "crates/a/src/x.realm.json", 4)
            self.assertEqual(len(violations(root, allowed)), 1)

    def test_missing_repository_is_an_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                companions(Path(temporary))


if __name__ == "__main__":
    unittest.main()
