#!/usr/bin/env python3
import unittest
from realm_namespace_migration import namespace_factories


class NamespaceMigrationTests(unittest.TestCase):
    def test_factory_uses_its_actual_ordered_member_declaration(self):
        source='const MEMBERS:&[Member]=&[Member::method("x",fn_info!(x,0),0),Member::tag("Example")];\n\n#[cold]\nfn create(){builtin_namespace::create(MEMBERS)}\n'
        factories=namespace_factories(source)
        self.assertEqual(len(factories),1)
        self.assertEqual(factories[0]["name"],"Example")
        self.assertEqual(factories[0]["line"],2)
        self.assertEqual(factories[0]["table"],"MEMBERS")
        self.assertEqual(namespace_factories(source.replace("create(MEMBERS)","create(OTHER)")),[])


if __name__=="__main__":
    unittest.main()
