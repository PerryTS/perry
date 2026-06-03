import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "parity_matrix_trend.py"


class ParityMatrixTrendTests(unittest.TestCase):
    def write_json(self, path: Path, data: dict) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data), encoding="utf-8")

    def write_output(self, root: Path, test_id: str, node: str, perry: str) -> None:
        safe = test_id.replace("/", "__")
        node_path = root / "output" / "node" / f"{safe}.txt"
        perry_path = root / "output" / "perry" / f"{safe}.txt"
        node_path.parent.mkdir(parents=True, exist_ok=True)
        perry_path.parent.mkdir(parents=True, exist_ok=True)
        node_path.write_text(node, encoding="utf-8")
        perry_path.write_text(perry, encoding="utf-8")

    def run_checker(self, root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--report",
                str(root / "report.json"),
                "--known",
                str(root / "known_failures.json"),
                "--baseline",
                str(root / "baseline.json"),
                "--output-dir",
                str(root / "output"),
                "--output-json",
                str(root / "matrix.json"),
                "--output-md",
                str(root / "matrix.md"),
                "--check",
            ],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )

    def test_known_baselined_failure_passes(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self.write_json(root / "report.json", {
                "results": [
                    {"id": "test_parity_path", "status": "pass"},
                    {"id": "test_parity_zlib", "status": "parity_fail"},
                ],
                "failures": {"parity": ["test_parity_zlib"], "compile": []},
            })
            self.write_json(root / "known_failures.json", {
                "test_parity_zlib": {"category": "module-inventory", "reason": "known"}
            })
            self.write_json(root / "baseline.json", {
                "modules": {
                    "zlib": {
                        "allowed_statuses": ["pass", "parity_fail"],
                        "max_diff_lines": 4,
                    }
                }
            })
            self.write_output(root, "test_parity_path", "ok\n", "ok\n")
            self.write_output(root, "test_parity_zlib", "node\n", "perry\n")

            result = self.run_checker(root)

            self.assertEqual(result.returncode, 0, result.stdout)
            matrix = json.loads((root / "matrix.json").read_text(encoding="utf-8"))
            self.assertEqual(matrix["summary"]["modules"], 2)
            zlib = next(item for item in matrix["modules"] if item["module"] == "zlib")
            self.assertTrue(zlib["known"])
            self.assertEqual(zlib["diff_lines"], 2)

    def test_new_untriaged_failure_fails(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self.write_json(root / "report.json", {
                "results": [{"id": "test_parity_path", "status": "parity_fail"}],
                "failures": {"parity": ["test_parity_path"], "compile": []},
            })
            self.write_json(root / "known_failures.json", {})
            self.write_json(root / "baseline.json", {"modules": {}})
            self.write_output(root, "test_parity_path", "node\n", "perry\n")

            result = self.run_checker(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("test_parity_path", result.stdout)
            self.assertIn("not listed in known_failures.json", result.stdout)

    def test_diff_lines_regression_fails_even_for_known_failure(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self.write_json(root / "report.json", {
                "results": [{"id": "test_parity_zlib", "status": "parity_fail"}],
                "failures": {"parity": ["test_parity_zlib"], "compile": []},
            })
            self.write_json(root / "known_failures.json", {
                "test_parity_zlib": {"category": "module-inventory", "reason": "known"}
            })
            self.write_json(root / "baseline.json", {
                "modules": {
                    "zlib": {
                        "allowed_statuses": ["pass", "parity_fail"],
                        "max_diff_lines": 1,
                    }
                }
            })
            self.write_output(root, "test_parity_zlib", "node\n", "perry\n")

            result = self.run_checker(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("diff_lines 2 exceeds baseline 1", result.stdout)


if __name__ == "__main__":
    unittest.main()
