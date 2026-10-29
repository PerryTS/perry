import unittest
from realm_plain_namespace_migration import expression_in_function
class PlainNamespaceMigrationTests(unittest.TestCase):
    def test_only_the_selected_birth_is_replaced(self):
        source='fn other(){js_object_alloc(0, 0)}\nfn selected(){ let object=js_object_alloc(0, 0); }'
        a,b=expression_in_function(source,'selected','js_object_alloc(0, 0)')
        self.assertEqual(source[a:b],'js_object_alloc(0, 0)')
        self.assertGreater(a,source.index('selected'))
        with self.assertRaises(AssertionError):
            expression_in_function(source.replace('let object=','js_object_alloc(0, 0);let object='),'selected','js_object_alloc(0, 0)')
