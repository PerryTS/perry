import unittest
from realm_native_key_migration import parent_keys
class NativeKeyMigrationTests(unittest.TestCase):
    def test_only_parent_result_keys_are_births(self):
        s='fn native_parent(m:&str,n:&str){Some(match(m,n){("a","#First")=>("b","#Second"),("c","A"|"B")=>("d","#Third"),_=>return None})}'
        found=parent_keys(s)
        self.assertEqual(set(found),{('b','#Second'),('d','#Third')})
        self.assertEqual(s[found['b','#Second'][1]:found['b','#Second'][2]],'"#Second"')
