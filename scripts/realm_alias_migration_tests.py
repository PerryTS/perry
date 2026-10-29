import unittest
from realm_alias_migration import alias_facts
import realm_source_migration as m

class AliasMigrationTests(unittest.TestCase):
    def test_public_name_and_canonical_cell_are_distinct(self):
        d=dict(name='String.prototype.trimLeft',kind='method',captures=[],properties=[
            dict(key='name',value=dict(kind='string',value='trimStart'))])
        seen=[]
        def simple(d):
            seen.append(d['name'])
            return {}
        facts=alias_facts(d,simple)
        self.assertEqual(seen,['String.prototype.trimStart'])
        self.assertEqual(m.render(d['name'],facts),'String.prototype.trimStart, alias = String.prototype.trimLeft')
        self.assertEqual(d['name'],'String.prototype.trimLeft')
        self.assertIsNone(alias_facts(dict(d,captures=[{}]),simple))
