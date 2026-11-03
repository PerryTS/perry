import unittest
from realm_static_argument_migration import classify,NAMES
class StaticArgumentMigrationTests(unittest.TestCase):
    def test_selector_stays_at_birth_and_only_exceptional_length_is_projected(self):
        d={'name':NAMES[0],'length':1,'captures':[{'kind':'raw_word','value':'0x0'}],
           'function_info':{'params':5,'flags':0},'properties':[{'key':'length','value':{'value':'3ff0000000000000'}}]}
        def original(candidate):
            self.assertEqual(candidate['captures'],[])
            self.assertEqual(candidate['length'],5)
            return {}
        human=classify(d,original)
        self.assertEqual(human['capture'],'[number_argument(own_static, kind)]')
        self.assertEqual(human['spec'],'{length = 1}')
        self.assertIsNone(classify({'name':'Unrelated'},original))
