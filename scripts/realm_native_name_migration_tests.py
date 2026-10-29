import unittest
import realm_native_name_migration as n
class NativeNameMigrationTests(unittest.TestCase):
    def test_display_name_normalization_does_not_change_dispatch_binding(self):
        old=n.m.native_default_facts;seen=[]
        n.m.native_default_facts=lambda d:seen.append(d) or dict(receiver='Module')
        try:
            d=dict(name='node:fs.Stats',function_info=dict(native_method=True),length=18,captures=['Stats'],properties=[dict(key='name',value=dict(value='deprecated')),dict(key='length',value=dict(value='x'))])
            f,label=n.name_facts(d)
            self.assertEqual(label,'deprecated');self.assertEqual(seen[0]['captures'],['Stats'])
            self.assertEqual(seen[0]['properties'][0]['value']['value'],'Stats')
            self.assertEqual(d['length'],18)
        finally:n.m.native_default_facts=old
