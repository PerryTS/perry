import unittest
from realm_enum_capture_migration import property_bindings

class EnumCaptureMigrationTests(unittest.TestCase):
    def test_uses_named_variants_and_rejects_ambiguous_rows(self):
        source='#[repr(u8)] enum Prop { Closed, Flowing, } let rows=[("flowing", Prop::Flowing), ("closed", Prop::Closed)];'
        self.assertEqual(property_bindings(source)['flowing'], [('Prop','0x3ff0000000000000')])
        self.assertNotIn('flowing',property_bindings(source+' ("flowing", Prop::Closed)'))
        self.assertEqual(property_bindings(source.replace('Closed,','Closed=2,')),{})
if __name__=='__main__':unittest.main()
