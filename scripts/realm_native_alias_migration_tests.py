import unittest
from realm_native_alias_migration import alias_facts
class NativeAliasMigrationTests(unittest.TestCase):
    def test_alias_uses_dispatch_name_without_mutating_input(self):
        d=dict(name='node:path._makeLong',captures=[{},dict(kind='bytes',value='toNamespacedPath'),{}],properties=[dict(key='name',value=dict(kind='string',value='toNamespacedPath'))])
        result=alias_facts(d,lambda x:dict(receiver=x['name']))
        self.assertEqual(result['birth_name'],'node:path.toNamespacedPath')
        self.assertEqual(result['alias'],d['name'])
        self.assertEqual(d['name'],'node:path._makeLong')
        d['properties'][0]['value']['value']='different'
        self.assertIsNone(alias_facts(d,lambda x:{}))
