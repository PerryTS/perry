#!/usr/bin/env python3
from pathlib import Path
import tempfile
import unittest
from check_no_realm_companions import companions


class NoCompanionTests(unittest.TestCase):
    def test_nested_source_companion_is_rejected_and_build_output_is_allowed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "crates/example/src/nested"
            source.mkdir(parents=True)
            (source / "body.rs").write_text("")
            output = root / "target/debug/build/runtime/out"
            output.mkdir(parents=True)
            (output / "realm-v1.json").write_text("{}")
            (output / "module.realm.json").write_text("{}")
            self.assertEqual(companions(root), [])
            forbidden = source / "body.realm.json"
            forbidden.write_text("{}")
            self.assertEqual(companions(root), [forbidden.relative_to(root)])

    def test_missing_repository_is_an_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                companions(Path(temporary))


if __name__ == "__main__":
    unittest.main()
