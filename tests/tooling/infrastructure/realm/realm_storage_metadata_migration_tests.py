import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts'))

import realm_storage_metadata_migration as s
class StorageMetadataMigrationTests(unittest.TestCase):
    def test_only_zero_length_installer_contract(self):
        old=s.human_length;s.human_length=lambda d:({},1)
        try:
            with self.assertRaises(ValueError):s.storage_facts(dict(name='Storage.prototype.getItem'))
            self.assertIsNone(s.storage_facts(dict(name='Unrelated.method')))
        finally:s.human_length=old
