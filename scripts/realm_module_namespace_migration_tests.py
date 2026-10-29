import unittest
from realm_module_namespace_migration import render
class ModuleNamespaceMigrationTests(unittest.TestCase):
    def test_intrinsic_tag_and_descriptors_are_not_source_facts(self):
        d=dict(name='node:sample',properties=[dict(key='member',symbol=False,attributes=3,value=dict(kind='reference',value='node:sample.member')),dict(key='@@toStringTag',symbol=True,attributes=0,value=dict(kind='string',value='Module'))],own_key_order=['member'],own_symbol_order=['@@toStringTag'],births=['node:sample.member'])
        self.assertEqual(render(d),'"node:sample", members = { member; }')
