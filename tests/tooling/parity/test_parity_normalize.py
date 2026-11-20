#!/usr/bin/env python3
"""Pin the `parity` rules of `scripts/parity_normalize.py`.

The fixtures in `tests/fixtures/parity_normalize/` were captured from the shell
chain these rules replaced, so passing them is the evidence that the port
changed no verdict.

The inventory test asserts the fixtures are live. A golden-file suite whose
subject silently empties out otherwise reports the same green verdict for "all
cases match" and "there were no cases".
"""

import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
FIXTURES = os.path.join(ROOT, "tests", "fixtures", "parity_normalize")

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

    def test_trim_sits_between_the_two_delete_stages(self):
        # The historical chain trimmed trailing whitespace BEFORE its
        # `$`-anchored console-warning delete but AFTER its prefix-anchored
        # ones, so the two directions are separately observable:
        #   - padding HIDES a warning from the delete (it must survive only if
        #     the delete moves back into the shared, earlier stage);
        #   - a padded `Debugger listening …` line still SURVIVES, because its
        #     delete ran before the trim ever happened.
        # Moving the trim above the whole delete stage satisfies the first
        # line and breaks the second, which is exactly why this assertion pins
        # both. An independent review found this ordering unpacked by the
        # fixture set; this is the assertion that now covers it.
        text = (
            "Warning: Count for label 'x' does not exist   \n"
            "Debugger listening on ws://127.0.0.1:9229/abc   \n"
            "kept   \n"
        )
        self.assertEqual(
            parity_normalize.normalize_parity_output(text),
            "Debugger listening on ws://127.0.0.1:9229/abc\nkept\n",
        )


class FixtureInventoryTest(unittest.TestCase):
    def test_inventory_is_live(self):
        counts = {
            "output": len(_cases("output__")),
            "failure": len(_cases("failure__")),
        }
        self.assertGreaterEqual(counts["output"], 15, counts)
        self.assertGreaterEqual(counts["failure"], 5, counts)

    def test_every_case_has_both_files(self):
        stems = {name.rsplit(".", 1)[0] for name in os.listdir(FIXTURES)}
        for stem in stems:
            with self.subTest(stem=stem):
                self.assertTrue(os.path.exists(os.path.join(FIXTURES, stem + ".in")), stem)
                self.assertTrue(
                    os.path.exists(os.path.join(FIXTURES, stem + ".expected")), stem
                )


if __name__ == "__main__":
    unittest.main()
