"""Regression coverage for the per-PR cargo-test package selection."""
import contextlib
import io
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
import ci_test_scope as scope


def package(name, *dependencies):
    return {
        "name": name,
        "manifest_path": f"/workspace/crates/{name}/Cargo.toml",
        "dependencies": [{"name": dep} for dep in dependencies],
    }


class CargoTestScopeTests(unittest.TestCase):
    def setUp(self):
        self.metadata = {"packages": [
            package("perry-runtime"),
            package("perry-stdlib", "perry-runtime"),
            package("perry-stdlib-static", "perry-stdlib"),
            package("perry-ffi", "perry-runtime"),
            package("perry", "perry-ffi"),
            package("perry-ext-net", "perry-runtime", "perry-stdlib"),
            package("perry-parser"),
        ]}

    def selected(self, *paths, full=False):
        output = io.StringIO()
        argv = ["ci_test_scope.py"] + (["--full"] if full else [])
        with patch.object(scope, "_load_metadata", return_value=self.metadata), \
                patch.object(sys, "argv", argv), \
                patch.object(sys, "stdin", io.StringIO("\n".join(paths))), \
                contextlib.redirect_stdout(output):
            self.assertEqual(scope.main(), 0)
        return set(output.getvalue().splitlines())

    def test_library_target_detection(self):
        for kind in ("lib", "rlib", "staticlib", "dylib", "cdylib", "proc-macro",
                     "bin", "test", "example", "bench", "custom-build"):
            with self.subTest(kind=kind):
                md = {"packages": [{"name": "subject", "targets": [{"kind": [kind]}]}]}
                with patch.object(scope, "_load_metadata", return_value=md), \
                        patch.object(sys, "stdin", io.StringIO("subject\n")):
                    expected = 0 if kind in {
                        "lib", "rlib", "staticlib", "dylib", "cdylib", "proc-macro"
                    } else 1
                    self.assertEqual(scope._has_lib_mode(), expected)

    def test_runtime_changes_run_stdlib_tests(self):
        for path in ("arena/block.rs", "gc/roots.rs", "closure/mod.rs",
                     "event_pump.rs", "promise.rs"):
            with self.subTest(path=path):
                selected = self.selected(f"crates/perry-runtime/src/{path}")
                self.assertIn("perry-stdlib", selected)
                self.assertIn("perry-runtime", selected)
                self.assertIn("perry", selected)
                self.assertNotIn("perry-ext-net", selected)
                self.assertNotIn("perry-parser", selected)

    def test_stdlib_direct_change_selects_stdlib_and_driver(self):
        selected = self.selected("crates/perry-stdlib/src/common/async_bridge.rs")
        self.assertIn("perry-stdlib", selected)
        self.assertIn("perry", selected)
        self.assertNotIn("perry-ext-net", selected)

    def test_extension_direct_change_is_still_selected(self):
        self.assertEqual(self.selected("crates/perry-ext-net/src/lib.rs"),
                         {"perry-ext-net", "perry"})

    def test_unrelated_change_does_not_run_stdlib(self):
        self.assertEqual(self.selected("crates/perry-parser/src/lib.rs"),
                         {"perry-parser"})

    def test_metadata_only_selects_nothing(self):
        self.assertEqual(self.selected("Cargo.toml", "Cargo.lock", "CLAUDE.md"), set())

    def test_full_and_infra_still_include_extensions(self):
        expected = {p["name"] for p in self.metadata["packages"]} - scope.EXCLUDED
        self.assertEqual(self.selected(full=True), expected)
        self.assertEqual(self.selected("scripts/ci_test_scope.py"), expected)


if __name__ == "__main__":
    unittest.main()
