import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts'))

from realm_remaining_length_migration import replacement
class RemainingLengthMigrationTests(unittest.TestCase):
    def test_replacement_is_unique_and_keeps_provenance_lines(self):
        self.assertEqual(replacement('a\nlength(1)\nb','length(1)','length(meta!(length))'),'a\nlength(meta!(length))\nb')
        with self.assertRaises(AssertionError):replacement('x x','x','y')
        with self.assertRaises(AssertionError):replacement('x','x','y\n')
