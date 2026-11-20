import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts'))

from realm_remaining_name_migration import replace_literal
class NameMigrationTests(unittest.TestCase):
    def test_only_unique_same_line_projection_is_allowed(self):
        self.assertEqual(replace_literal('x\n"name"\ny','"name"','metadata!(name)'), 'x\nmetadata!(name)\ny')
        with self.assertRaises(AssertionError):replace_literal('x x','x','y')
        with self.assertRaises(AssertionError):replace_literal('x','x','y\n')
