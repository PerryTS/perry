import tempfile
from pathlib import Path
import tomllib
import unittest

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))

from component_guest_acceptance import prepare_rust_host_manifest
from wasi_host_qualification import (
    COMMAND_PHASES, P2_PHASES, complete_phases, discover, moving_witness, unit_results, verdict,
)


class ComponentGuestManifestTests(unittest.TestCase):
    def test_copied_fixture_resolves_the_workspace_dependency(self):
        root = Path(__file__).resolve().parents[3]
        fixture = root / "tests/integration/wasi/components/transform/rust-host/Cargo.toml"
        original = fixture.read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="perry component guest ") as directory:
            copied = Path(directory) / "Cargo.toml"
            copied.write_text(original, encoding="utf-8")
            prepare_rust_host_manifest(copied, root)
            expected = tomllib.loads(original)
            expected["dependencies"]["perry-wasi"]["path"] = str(root / "crates/perry-wasi")
            self.assertEqual(tomllib.loads(copied.read_text(encoding="utf-8")), expected)
            self.assertTrue((root / "crates/perry-wasi/Cargo.toml").is_file())
        self.assertEqual(fixture.read_text(encoding="utf-8"), original)

    def test_manifest_without_the_fixture_dependency_is_rejected(self):
        root = Path(__file__).resolve().parents[3]
        with tempfile.TemporaryDirectory() as directory:
            copied = Path(directory) / "Cargo.toml"
            original = '[dependencies]\nperry-wasi = { path = "elsewhere" }\n'
            copied.write_text(original, encoding="utf-8")
            with self.assertRaisesRegex(AssertionError, "Rust host path changed: crates/perry-wasi"):
                prepare_rust_host_manifest(copied, root)
            self.assertEqual(copied.read_text(encoding="utf-8"), original)


class QualificationTests(unittest.TestCase):
    def test_incomplete_or_duplicate_component_phases_cannot_pass(self):
        for expected, count in [(P2_PHASES, 28), (COMMAND_PHASES, 9)]:
            self.assertEqual(len(expected), count)
            rows = [{"name": name, "exitCode": 0} for name in sorted(expected)]
            self.assertTrue(complete_phases(rows, expected))
            self.assertFalse(complete_phases(rows[:-1], expected))
            self.assertFalse(complete_phases(rows[:-1] + rows[:1], expected))
            self.assertFalse(complete_phases(rows + rows[:1], expected))
            rows[0]["exitCode"] = 1
            self.assertFalse(complete_phases(rows, expected))

    def test_unit_evidence_requires_both_complete_host_suites(self):
        def suite(package, count, failed=0):
            return (f"Running unittests src/lib.rs (target/perry-dev/{package}-abc.exe)\n"
                    f"test result: ok. {count} passed; {failed} failed; 0 ignored; 0 measured; 0 filtered out;\n")
        component = suite("perry_component_host", 19)
        wasm = suite("perry_wasm_host", 26)
        self.assertEqual(unit_results(component + wasm),
                         {"perry_component_host": 19, "perry_wasm_host": 26})
        for bad in [component, component + component + wasm,
                    component + suite("perry_wasm_host", 25, 1),
                    component + wasm.replace("0 filtered", "1 filtered")]:
            with self.assertRaises(ValueError):
                unit_results(bad)

    def test_matching_unexpected_failures_cannot_pass(self):
        self.assertTrue(verdict(0, 1, b"", 0, 1, b""))
        self.assertTrue(verdict(0, 0, b"ok\n", 1, None, b"ok\n"))
        self.assertTrue(verdict(0, 0, b"ok\n", 0, -6, b"ok\n"))

    def test_explicit_guest_exit_requires_both_statuses_and_output(self):
        self.assertEqual(verdict(7, 7, b"before\n", 0, 7, b"before\r\n"), [])
        self.assertTrue(verdict(7, 7, b"before\n", 0, 0, b"before\n"))
        self.assertTrue(verdict(7, 7, b"before\n", 0, 7, b"before\nafter\n"))

    def test_collection_count_alone_cannot_pass_moving_qualification(self):
        copied = b"[gc-copy-minor] ran copied_objects=5\n"
        verified = b"[gc-verify] evacuation_ok\n"
        quarantine = b"[gc-fromspace-protect] mode=ProtectPages retired_set=2\n"
        self.assertFalse(moving_witness(copied + verified)["passed"])
        self.assertFalse(moving_witness(verified + quarantine)["passed"])
        self.assertTrue(moving_witness(copied + verified + quarantine)["passed"])

    def test_discovery_excludes_import_helpers_and_type_declarations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ["case.ts", "nested/case.ts", "nested/fixtures/helper.ts", "types.d.ts"]:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("", encoding="utf-8")
            self.assertEqual([p.relative_to(root).as_posix() for p in discover(root)],
                             ["case.ts", "nested/case.ts"])


if __name__ == "__main__":
    unittest.main()
