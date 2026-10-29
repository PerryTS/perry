import unittest
from realm_native_object_birth_migration import render
class NativeObjectBirthMigrationTests(unittest.TestCase):
    def test_virtual_members_use_native_defaults_and_preserve_edges(self):
        d=dict(name='node:example.default',prototype='Object.prototype',
               properties=[dict(key='method',symbol=False,attributes=7,
                                value=dict(kind='reference',value='node:example.method')),
                           dict(key='constant',symbol=False,attributes=0,
                                value=dict(kind='string',value='fixed'))],
               own_key_order=['method','constant'],own_symbol_order=[],
               births=['Object.prototype','node:example.method','Other'])
        result=render(d)
        self.assertIn('method = ref("node:example.method");',result)
        self.assertIn('constant = "fixed", attrs = none',result)
        self.assertIn('births = [Other]',result)
        for derived in ('header','class_id','reserved','Object.prototype'):
            self.assertNotIn(derived,result)
