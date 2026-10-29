import unittest
from realm_record_migration import locate
class RecordMigrationTests(unittest.TestCase):
    def test_the_layout_identifies_the_existing_builder_without_offsets(self):
        source='prefix\ncrate::object::runtime_layouts::runtime_record!(config, RECORD_X);\nsuffix'
        match=locate(source,'RECORD_X')
        self.assertEqual(match[1],'config')
        self.assertEqual(source[:match.start()].count('\n'),1)
        with self.assertRaises(AssertionError):locate(source+source,'RECORD_X')
        with self.assertRaises(AssertionError):locate(source,'OTHER')
