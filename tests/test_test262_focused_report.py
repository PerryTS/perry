import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "test262_focused_report.py"

SPEC = importlib.util.spec_from_file_location("test262_focused_report", SCRIPT_PATH)
assert SPEC is not None
REPORT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(REPORT)


class Test262FocusedReportTests(unittest.TestCase):
    def test_problem_rows_extracts_only_problem_buckets(self):
        report = {
            "totals": {
                "pass": 10,
                "diff": 1,
                "runtime-fail": 1,
                "compile-fail": 1,
                "skip": 1,
            },
            "samples": {
                "diff": [{"test": "language/expressions/a.js", "reason": "stdout"}],
                "runtime-fail": [
                    {
                        "test": "language/expressions/b.js",
                        "reason": "TypeError\nextra detail",
                    }
                ],
                "compile-fail": [
                    {
                        "test": "language/expressions/c.js",
                        "reason": "unsupported\tassignment",
                    }
                ],
                "skip": [{"test": "language/expressions/skip.js", "reason": "host"}],
            },
        }

        rows = REPORT.problem_rows(report)

        self.assertEqual(
            rows,
            [
                ("diff", "language/expressions/a.js", "stdout"),
                ("runtime-fail", "language/expressions/b.js", "TypeError extra detail"),
                ("compile-fail", "language/expressions/c.js", "unsupported assignment"),
            ],
        )

    def test_write_problem_tsv_requires_complete_samples(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "report.json"
            path.write_text(
                json.dumps(
                    {
                        "totals": {
                            "pass": 0,
                            "diff": 2,
                            "runtime-fail": 0,
                            "compile-fail": 0,
                            "skip": 0,
                        },
                        "samples": {
                            "diff": [
                                {
                                    "test": "language/expressions/a.js",
                                    "reason": "stdout",
                                }
                            ],
                            "runtime-fail": [],
                            "compile-fail": [],
                            "skip": [],
                        },
                    }
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "incomplete"):
                REPORT.write_problem_tsv(path, Path(temp) / "problems.tsv")

    def test_write_problem_tsv_emits_header(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "report.json"
            out = Path(temp) / "problems.tsv"
            path.write_text(
                json.dumps(
                    {
                        "totals": {
                            "pass": 0,
                            "diff": 0,
                            "runtime-fail": 1,
                            "compile-fail": 0,
                            "skip": 0,
                        },
                        "samples": {
                            "diff": [],
                            "runtime-fail": [
                                {
                                    "test": "language/expressions/b.js",
                                    "reason": "TypeError",
                                }
                            ],
                            "compile-fail": [],
                            "skip": [],
                        },
                    }
                )
                + "\n",
                encoding="utf-8",
            )

            count = REPORT.write_problem_tsv(path, out)

            self.assertEqual(count, 1)
            self.assertEqual(
                out.read_text(encoding="utf-8"),
                "bucket\ttest\treason\n"
                "runtime-fail\tlanguage/expressions/b.js\tTypeError\n",
            )


if __name__ == "__main__":
    unittest.main()
