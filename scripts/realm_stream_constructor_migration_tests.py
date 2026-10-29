import unittest
from realm_stream_constructor_migration import classify
class StreamConstructorMigrationTests(unittest.TestCase):
    def test_only_public_length_changes_and_runtime_projection_is_required(self):
        d={'name':'WritableStream','length':0,'function_info':{'params':1,'flags':0},
           'properties':[{'key':'length','value':{'value':'0000000000000000'}}]}
        def original(candidate):
            self.assertEqual(candidate['length'],1)
            return {'provider':'builtin_constructor'}
        facts=classify(d,original)
        self.assertEqual(facts['spec'],'{length = 0}')
        self.assertEqual(facts['emit'],'metadata(realm_stream_constructor_public)')
