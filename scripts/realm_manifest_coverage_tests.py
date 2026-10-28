import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import realm_manifest_coverage as coverage


class CoverageTests(unittest.TestCase):
    def test_a_new_namespace_member_requires_its_own_declaration(self):
        rows = [
            {"path": "$global", "properties": [{"key": "Temporal", "descriptor": {"value": {"ref": "$t"}}}],
             "prototype": None},
            {"path": "$t", "properties": [{"key": "Duration", "descriptor": {"value": {"ref": "$d"}}}],
             "prototype": None},
            {"path": "$d", "properties": [], "prototype": None},
        ]
        manifest = {"declarations": [{"name": "%Global%"}, {"name": "Temporal"}]}
        with self.assertRaisesRegex(ValueError, "Temporal.Duration"):
            coverage.check(rows, manifest)
        manifest["declarations"].append({"name": "Temporal.Duration"})
        self.assertEqual(coverage.check(rows, manifest), (3, 3))

    def test_unreachable_cells_fail_instead_of_disappearing(self):
        rows = [{"path": "$global", "properties": [], "prototype": None},
                {"path": "$orphan", "properties": [], "prototype": None}]
        with self.assertRaisesRegex(ValueError, "unreachable"):
            coverage.cell_names(rows)


if __name__ == "__main__":
    unittest.main()
