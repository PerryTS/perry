import unittest
from realm_inherited_birth_migration import addition
class InheritedBirthMigrationTests(unittest.TestCase):
    def test_existing_birth_gets_an_entry_without_a_nested_allocator(self):
        s='let x = crate::realm_decl! { birth alloc(); realm { A, members = { }; } };'
        a=s.index('alloc()');b=a+len('alloc()')
        start,end,replacement,line=addition(s,a,b,['B, members = { }'])
        changed=s[:start]+replacement+s[end:]
        self.assertEqual(changed.count('realm_decl!'),1)
        self.assertEqual(changed.count('alloc()'),1)
        self.assertIn('A, members = { }; B, members = { };',changed)
        self.assertEqual(line,1)
    def test_new_birth_keeps_expression_and_uses_owner_crate(self):
        start,end,replacement,line=addition('\nalloc()',1,8,['A, members = { }'],'perry_runtime')
        self.assertTrue(replacement.startswith('perry_runtime::realm_decl! { birth alloc();'))
        self.assertEqual((start,end,line),(1,8,2))
