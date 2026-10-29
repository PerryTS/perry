import unittest
from realm_agent_capture_migration import classify
class AgentCaptureMigrationTests(unittest.TestCase):
    def test_raw_receiver_binding_retains_owner_without_duplicate_edge(self):
        d={'name':'node:http.globalAgent.<prototype>.addRequest','body':'x::global_agent_add_request_thunk',
           'captures':[{'kind':'raw_reference','value':'node:http.globalAgent'}],'length':4,
           'function_info':{'params':5,'flags':0},'properties':[]}
        def original(candidate):
            self.assertEqual(candidate['captures'],[])
            return {'births':['node:http.globalAgent']}
        facts=classify(d,original)
        self.assertEqual(facts['capture'],'[raw_ref("node:http.globalAgent")]')
        self.assertNotIn('births',facts)
        self.assertEqual(facts['spec'],'{length = 4}')
