import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts'))

from realm_empty_array_migration import field_keys
class EmptyArrayMigrationTests(unittest.TestCase):
    def test_keys_follow_the_existing_record_layout(self):
        s='runtime_atom!(A,"first",""); runtime_atom!(B,"second",""); runtime_layout!(RECORD,1,0,P,[B,A]);'
        self.assertEqual(field_keys(s,'RECORD'),['second','first'])
        with self.assertRaises(ValueError):field_keys(s,'MISSING')
