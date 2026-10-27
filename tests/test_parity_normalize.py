#!/usr/bin/env python3
"""Pin `scripts/parity_normalize.py` against the implementations it replaced.

The module exists because two parity runners each owned a private answer to
"did Perry print what Node printed", written twice in two languages, each
missing rules the other had. The fixtures in
`tests/fixtures/parity_normalize/` were captured from those two
implementations *before* the refactor, so passing this suite is the evidence
that unifying them changed no verdict in either lane.

The last test asserts the fixture inventory is live. A golden-file suite whose
subject silently empties out otherwise reports the same green verdict for "all
38 cases match" and "there were no cases".
"""

import os
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
FIXTURES = os.path.join(HERE, "fixtures", "parity_normalize")
MODULE = os.path.join(ROOT, "scripts", "parity_normalize.py")

sys.path.insert(0, os.path.join(ROOT, "scripts"))

import parity_normalize  # noqa: E402  (needs the sys.path entry above)


def _cases(prefix):
    names = sorted(
        name[: -len(".in")]
        for name in os.listdir(FIXTURES)
        if name.startswith(prefix) and name.endswith(".in")
    )
    return [(name, os.path.join(FIXTURES, name + ".in"),
             os.path.join(FIXTURES, name + ".expected")) for name in names]


def _read(path):
    with open(path, encoding="utf-8", newline="") as handle:
        return handle.read()


class ParityProfileTest(unittest.TestCase):
    def test_output_fixtures(self):
        for name, source, expected in _cases("output__"):
            with self.subTest(case=name):
                self.assertEqual(
                    parity_normalize.normalize_parity_output(_read(source)),
                    _read(expected),
                )

    def test_failure_fixtures(self):
        for name, source, expected in _cases("failure__"):
            with self.subTest(case=name):
                self.assertEqual(
                    parity_normalize.normalize_parity_failure(_read(source)),
                    _read(expected),
                )


class NodeSuiteProfileTest(unittest.TestCase):
    def test_fixtures(self):
        for name, source, expected in _cases("node_suite__"):
            with self.subTest(case=name):
                self.assertEqual(
                    parity_normalize.normalize_node_suite_output(_read(source)),
                    _read(expected),
                )


class DispatchTest(unittest.TestCase):
    def test_default_profile_is_parity(self):
        text = "true\n"
        self.assertEqual(
            parity_normalize.normalize_output(text),
            parity_normalize.normalize_parity_output(text),
        )

    def test_profiles_select_different_rules(self):
        # `true` folds to `1` only in the parity profile, and the node:test
        # reporter line is masked only in the node_suite profile. The parity
        # chain deliberately keeps that line: its `^\s+at ` / `^\s+[0-9]+: `
        # deletions do not match `    test at /x/…`, while `node_suite_run.py`
        # exists precisely to mask it. Neither rule set is safe in the other
        # lane, which is why the profiles are declared separately instead of
        # merged.
        text = "true\n    test at /x/a.test.ts:1:1\n"
        parity = parity_normalize.normalize_output(text, "parity")
        node_suite = parity_normalize.normalize_output(text, "node_suite")
        self.assertIn("1", parity)
        self.assertIn("test at /x/a.test.ts:1:1", parity)
        self.assertIn("test at <location>", node_suite)
        self.assertIn("true", node_suite)

    def test_unknown_profile_is_an_error(self):
        with self.assertRaises(ValueError):
            parity_normalize.normalize_output("x", "nope")


class CliTest(unittest.TestCase):
    """The shell runner drives the module as a process, so pin that contract."""

    def _run(self, *args, stdin=""):
        return subprocess.run(
            [sys.executable, MODULE, *args],
            input=stdin, capture_output=True, text=True,
        )

    def test_output_mode_reads_stdin(self):
        proc = self._run("output", stdin="true\n")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout, "1\n")

    def test_output_mode_honours_profile(self):
        # `node_suite_run.py` hands this profile `stdout.rstrip("\n")`, so the
        # production shape has no trailing newline and neither does the result.
        proc = self._run("output", "--profile", "node_suite", stdin="ℹ duration_ms 12")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout, "ℹ duration_ms <test-duration>")

    def test_failure_mode(self):
        proc = self._run("failure", stdin="node:internal/x:1\nError: boom\n    at f (/x:1)\n")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout, "Error: boom")

    def test_failure_mode_rejects_other_profiles(self):
        proc = self._run("failure", "--profile", "node_suite", stdin="x\n")
        self.assertEqual(proc.returncode, 2)

    def test_usage_errors(self):
        for args in ((), ("nonsense",), ("output", "--profile")):
            with self.subTest(args=args):
                self.assertEqual(self._run(*args).returncode, 2)


class FixtureInventoryTest(unittest.TestCase):
    def test_inventory_is_live(self):
        counts = {
            "output": len(_cases("output__")),
            "failure": len(_cases("failure__")),
            "node_suite": len(_cases("node_suite__")),
        }
        self.assertGreaterEqual(counts["output"], 15, counts)
        self.assertGreaterEqual(counts["failure"], 5, counts)
        self.assertGreaterEqual(counts["node_suite"], 4, counts)

    def test_every_case_has_both_files(self):
        stems = {name.rsplit(".", 1)[0] for name in os.listdir(FIXTURES)}
        for stem in stems:
            if stem.startswith("_"):
                continue
            with self.subTest(stem=stem):
                self.assertTrue(os.path.exists(os.path.join(FIXTURES, stem + ".in")), stem)
                self.assertTrue(
                    os.path.exists(os.path.join(FIXTURES, stem + ".expected")), stem
                )


if __name__ == "__main__":
    unittest.main()
