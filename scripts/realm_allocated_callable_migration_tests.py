import unittest
import realm_allocated_callable_migration as a
class AllocatedCallableMigrationTests(unittest.TestCase):
    def test_only_uncaptured_intrinsic_allocation(self):
        old=a.m.CONSTANTS;a.m.CONSTANTS=dict(GC_LAYOUT_POINTER_FREE=16384)
        try:
            d=dict(captures=[],header=dict(reserved=16384))
            self.assertEqual(a.allocated_facts(d,lambda x:dict(constructible=True)),dict(constructible=True,provider='intrinsic_closure'))
            self.assertEqual(d['header']['reserved'],16384)
            d['captures']=[1]
            self.assertIsNone(a.allocated_facts(d,lambda x:{}))
        finally:a.m.CONSTANTS=old
