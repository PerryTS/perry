import unittest
from realm_remaining_length_migration import replacement
class RemainingLengthMigrationTests(unittest.TestCase):
    def test_replacement_is_unique_and_keeps_provenance_lines(self):
        self.assertEqual(replacement('a\nlength(1)\nb','length(1)','length(meta!(length))'),'a\nlength(meta!(length))\nb')
        with self.assertRaises(AssertionError):replacement('x x','x','y')
        with self.assertRaises(AssertionError):replacement('x','x','y\n')
