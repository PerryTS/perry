import unittest
from realm_event_capture_migration import classify
class EventCaptureMigrationTests(unittest.TestCase):
    def test_reference_capture_owns_the_getter_edge(self):
        name='node:events.EventEmitter.defaultMaxListeners.<set>'
        d={'name':name,'captures':[{'kind':'reference','value':name.replace('<set>','<get>')}]}
        def original(candidate):
            self.assertEqual(candidate['captures'],[])
            return {'provider':'closure','births':['getter']}
        human=classify(d,original)
        self.assertNotIn('births',human)
        self.assertEqual(human['provider'],'closure')
        self.assertIn('<get>',human['capture'])
