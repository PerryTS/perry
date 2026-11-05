"""Exercise relocated coverage tools through their command-line interfaces."""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


def make_fixture(root: Path) -> None:
    files = {
        "crates/perry-runtime/src/lib.rs": (
            'pub extern "C" fn runtime_missing() {}\n'
            'pub extern "C" fn ts_covered() {}\n'
            'pub unsafe extern "C" fn tsx_covered() {}\n'
            'pub extern "C" fn rust_covered() {}\n'
        ),
        "crates/perry-runtime/src/tui/widget.rs": 'pub extern "C" fn ui_missing() {}\n',
        "crates/perry-stdlib/src/utils.rs": 'pub extern "C" fn stdlib_missing() {}\n',
        "crates/perry-stdlib/src/http.rs": 'pub extern "C" fn io_missing() {}\n',
        "crates/perry-stdlib/src/mongodb.rs": 'pub extern "C" fn integration_missing() {}\n',
        "crates/perry-runtime/tests/check.rs": "#[test]\nfn covers() { rust_covered(); }\n",
        "test-files/behavior.ts": "// @covers ts_covered\n",
        "test-files/behavior.tsx": "// @covers tsx_covered\n",
        "test-files/test_ffi_surface_coverage.ts": "// old inventory marker\n",
        "tests/coverage/COVERAGE.md": "existing report\n",
    }
    for name, content in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)


class CoverageLayoutTests(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory(prefix="perry-coverage-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        make_fixture(self.root)
        for name in ("audit.sh", "regen_ts_surface_inventory.py"):
            shutil.copy2(ROOT / "tests/coverage" / name, self.root / "tests/coverage" / name)

    def run_tool(self, name: str, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [str(self.root / "tests/coverage" / name), *args],
            cwd=self.root / "test-files",
            capture_output=True,
            text=True,
            check=False,
        )

    def test_audit_finds_repository_sources_and_writes_report_beside_tool(self) -> None:
        result = self.run_tool("audit.sh")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "Scanning FFI functions...\nFound 8 FFI functions\n")
        self.assertIn("Total:              8\n", result.stdout)
        self.assertIn("TS covered:         2 (25.0%)\n", result.stdout)
        self.assertIn("Rust covered:       1 (12.5%)\n", result.stdout)
        self.assertIn("Combined covered:   3 (37.5%)\n", result.stdout)
        self.assertIn("Combined uncovered: 5\n", result.stdout)
        report = self.root / "tests/coverage/COVERAGE.md"
        self.assertEqual(report.read_text(), "existing report\n")

        result = self.run_tool("audit.sh", "--markdown")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(
            result.stderr,
            "Scanning FFI functions...\nFound 8 FFI functions\n"
            "TS coverage: 2/8 (25.0%)\nCombined coverage: 3/8 (37.5%)\n"
            f"Report written to: {report}\n",
        )
        markdown = report.read_text()
        self.assertRegex(markdown, r"Generated: \d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ\n")
        self.assertIn("- **Total FFI functions:** 8\n", markdown)
        self.assertIn("- **Covered by TypeScript fixtures:** 2 (25.0%)\n", markdown)
        self.assertIn("- **Covered by Rust tests:** 1 (12.5%)\n", markdown)
        self.assertIn("- **Covered by either TS or Rust:** 3 (37.5%)\n", markdown)
        self.assertIn("- **Uncovered by either TS or Rust:** 5\n", markdown)
        self.assertIn("- `runtime_missing` (crates/perry-runtime/src/lib.rs)\n", markdown)
        self.assertFalse((self.root / "COVERAGE.md").exists())

    def test_generator_keeps_inventory_bytes_and_legacy_fixture_destination(self) -> None:
        inventory = {
            "runtime_core": (
                "Runtime core FFI surface inventory.", "testFfiSurfaceRuntimeCore", 2,
                "crates/perry-runtime/src/lib.rs:\n  - runtime_missing\n  - rust_covered\n",
            ),
            "runtime_ui": (
                "Runtime UI and platform FFI surface inventory.", "testFfiSurfaceRuntimeUi", 1,
                "crates/perry-runtime/src/tui/widget.rs:\n  - ui_missing\n",
            ),
            "stdlib_core": (
                "Stdlib core utility FFI surface inventory.", "testFfiSurfaceStdlibCore", 1,
                "crates/perry-stdlib/src/utils.rs:\n  - stdlib_missing\n",
            ),
            "stdlib_io": (
                "Stdlib IO, network, stream, and framework FFI surface inventory.",
                "testFfiSurfaceStdlibIo", 1,
                "crates/perry-stdlib/src/http.rs:\n  - io_missing\n",
            ),
            "stdlib_integrations": (
                "Stdlib external integration FFI surface inventory.",
                "testFfiSurfaceStdlibIntegrations", 1,
                "crates/perry-stdlib/src/mongodb.rs:\n  - integration_missing\n",
            ),
        }
        fixtures = self.root / "test-files"
        stale = "// runtime_missing rust_covered ui_missing stdlib_missing io_missing integration_missing\n"
        (fixtures / "test_ffi_surface_coverage.ts").write_text(stale)
        for group in inventory:
            (fixtures / f"test_ffi_surface_{group}.ts").write_text(stale)

        result = self.run_tool("regen_ts_surface_inventory.py")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assertEqual(
            result.stdout,
            "wrote test-files/test_ffi_surface_runtime_core.ts (2 names)\n"
            "wrote test-files/test_ffi_surface_runtime_ui.ts (1 names)\n"
            "wrote test-files/test_ffi_surface_stdlib_core.ts (1 names)\n"
            "wrote test-files/test_ffi_surface_stdlib_io.ts (1 names)\n"
            "wrote test-files/test_ffi_surface_stdlib_integrations.ts (1 names)\n"
            "non-inventory TS declarations: 2/8\ninventory unique names: 6\n",
        )
        self.assertFalse((fixtures / "test_ffi_surface_coverage.ts").exists())
        expected_bytes = {}
        for group, (title, variable, count, entries) in inventory.items():
            filename = f"test_ffi_surface_{group}.ts"
            expected = (
                f"// {title}\n//\n"
                "// This fixture is intentionally executable by the normal parity runner,\n"
                "// but its main purpose is to keep TS-side coverage accounting attached\n"
                "// to related public FFI shims. Move @covers entries from this\n"
                "// inventory into behavioral tests as each area gets deeper compatibility\n"
                "// coverage.\n//\n"
                f"// Inventory entries: {count} unique FFI names, {count} declarations.\n\n"
                f"const {variable}Version = 1;\n"
                f"if ({variable}Version !== 1) {{\n"
                '  throw new Error("unexpected coverage inventory version");\n}\n'
                f'console.log("test_ffi_surface_{group}: ok");\n\n'
                f"/*\n@covers\n{entries}*/\n"
            ).encode()
            expected_bytes[filename] = expected
            self.assertEqual((fixtures / filename).read_bytes(), expected, filename)
        self.assertEqual((fixtures / "behavior.ts").read_text(), "// @covers ts_covered\n")
        self.assertEqual((fixtures / "behavior.tsx").read_text(), "// @covers tsx_covered\n")
        self.assertEqual(list((self.root / "tests/coverage").glob("*.ts")), [])

        repeated = self.run_tool("regen_ts_surface_inventory.py")
        self.assertEqual((repeated.returncode, repeated.stdout, repeated.stderr),
                         (result.returncode, result.stdout, result.stderr))
        self.assertEqual({name: (fixtures / name).read_bytes() for name in expected_bytes},
                         expected_bytes)


if __name__ == "__main__":
    unittest.main()
