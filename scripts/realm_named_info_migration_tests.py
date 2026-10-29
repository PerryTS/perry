#!/usr/bin/env python3
import unittest
import realm_source_migration as m
from realm_named_info_migration import named_infos


class NamedInfoMigrationTests(unittest.TestCase):
    def test_static_info_owns_abi_and_initializer_line_is_preserved(self):
        m.CONSTANTS={"FN_HAS_DECLARED":2048,"FN_REST_MASK":65543,"FN_REST_NATIVE_ARGS":65536}
        source="static EMIT: JsFunctionInfo =\n    JsFunctionInfo::of_native_args(emit,1).with_declared(2);\n"
        site=named_infos(source)[0]
        self.assertEqual(site["line"],1)
        self.assertEqual(site["binding"],"EMIT")
        self.assertEqual(site["signature"]["params"],0)
        self.assertEqual(site["signature"]["rest_fixed"],1)
        self.assertEqual(site["signature"]["declared"],2)
        self.assertEqual(named_infos("#[cfg(test)]\n"+source),[])
        visible="pub(super) "+source
        self.assertEqual(named_infos(visible)[0]["offset"],0)
        self.assertEqual(named_infos(visible)[0]["binding"],"EMIT")
        documented="/// Shared body info.\n"+source
        self.assertEqual(named_infos(documented)[0]["offset"],0)
        self.assertEqual(named_infos(documented)[0]["line"],1)


if __name__=="__main__":
    unittest.main()
