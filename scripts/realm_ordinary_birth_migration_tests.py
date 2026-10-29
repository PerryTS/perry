import unittest
from realm_ordinary_birth_migration import allocation
class OrdinaryBirthMigrationTests(unittest.TestCase):
    def test_selects_the_local_birth_in_the_named_function(self):
        s='fn other(){let proto = js_object_alloc(0,0);}\nfn owner(){let proto = js_object_alloc(0, 16);}'
        a,b=allocation(s,'owner','proto')
        self.assertEqual(s[a:b],'js_object_alloc(0, 16)')
        self.assertGreater(a,s.index('fn owner'))
        with self.assertRaises(ValueError):allocation(s,'owner','missing')
