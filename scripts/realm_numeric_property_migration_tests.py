import unittest
from realm_numeric_property_migration import selector
class NumericPropertyMigrationTests(unittest.TestCase):
    def test_word_and_float_bindings_are_distinct(self):
        source='[("state", 1.0), ("op", 2_i64)]'
        self.assertEqual(selector(source,'state',True),'0x3ff0000000000000')
        self.assertEqual(selector(source,'op',False),'0x2')
        self.assertIsNone(selector(source+' ("op",3)','op',False))
