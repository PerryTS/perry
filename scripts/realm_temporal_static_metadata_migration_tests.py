import unittest
from realm_temporal_static_metadata_migration import method_length
class TemporalStaticMetadataTests(unittest.TestCase):
    def test_only_from_and_compare_share_this_installer(self):
        self.assertEqual(method_length('from'),1);self.assertEqual(method_length('compare'),2)
        with self.assertRaises(KeyError):method_length('other')
