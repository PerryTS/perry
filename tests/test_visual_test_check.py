import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/run_visual_test_check.sh"


class VisualManifestTests(unittest.TestCase):
    def check_pair(self, labels, headings):
        with tempfile.TemporaryDirectory(prefix="visual-manifest-") as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            script = root / "scripts" / SCRIPT.name
            shutil.copyfile(SCRIPT, script)
            docs = root / "docs/examples/ui/styling"
            docs.mkdir(parents=True)
            (docs / "visual_test.ts").write_text(labels)
            (docs / "visual_test.spec.md").write_text(headings)
            return subprocess.run(
                ["bash", str(script)], capture_output=True, text=True, timeout=10,
            )

    def test_empty_pair_cannot_claim_coverage(self):
        result = self.check_pair("", "")
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("at least one numbered section", result.stdout)

    def test_existing_section_checks_are_preserved(self):
        for labels, headings, code in [
            ('labeled("1. One", row)\n', "### 1. One\n", 0),
            ('labeled("1. One", row)\n', "", 1),
            ('labeled("1. One", row)\nlabeled("1. Again", row)\n',
             "### 1. One\n### 2. Two\n", 1),
            ('labeled("2. Two", row)\n', "### 2. Two\n", 1),
        ]:
            with self.subTest(labels=labels, headings=headings):
                self.assertEqual(self.check_pair(labels, headings).returncode, code)


if __name__ == "__main__":
    unittest.main()
