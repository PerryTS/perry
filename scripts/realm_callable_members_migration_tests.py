import unittest
from realm_callable_members_migration import specialize
class CallableMembersMigrationTests(unittest.TestCase):
    def test_member_edges_are_implied_and_intrinsic_properties_are_not_copied(self):
        d=dict(name='A',kind='constructor',prototype='Function.prototype',function_info={},
               properties=[dict(key='prototype',symbol=False,value=dict(kind='reference',value='A.prototype'),attributes=0),
                           dict(key='from',symbol=False,value=dict(kind='reference',value='A.from'),attributes=5)],
               own_key_order=['length','name','prototype','from'],own_symbol_order=[])
        facts=specialize(d,lambda d:dict(births=['A.prototype','A.from','Promise']))
        self.assertEqual(facts['members'],'{ from; }')
        self.assertEqual(facts['births'],['Promise'])
