import unittest
from realm_owned_prototype_migration import eligible_native
class OwnedPrototypeMigrationTests(unittest.TestCase):
    def test_special_layout_is_not_placed_at_plain_allocator(self):
        d=dict(name='node:example.C.prototype',kind='prototype',header=dict(gc_type=2,reserved=2176,class_id=0),
               properties=[dict(key='constructor',value=dict(kind='reference',value='node:example.C'))])
        oracle={'node:example.C':dict(function_info=dict(native_method=True))}
        self.assertTrue(eligible_native(d,oracle))
        self.assertFalse(eligible_native(dict(d,name='node:events.EventEmitter.prototype'),oracle))
        self.assertFalse(eligible_native(d,{}))
