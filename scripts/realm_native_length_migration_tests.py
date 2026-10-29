import unittest
from realm_native_length_migration import arity_rows,arity_overrides
class NativeLengthMigrationTests(unittest.TestCase):
    def test_case_distinct_native_exports_keep_distinct_projections(self):
        from realm_native_length_migration import projection_name
        self.assertNotEqual(projection_name(("crypto","DiffieHellman")),projection_name(("crypto","diffieHellman")))

    def test_production_rows_and_explicit_override_are_both_found(self):
        rows=arity_rows('static CALLABLE_EXPORT_ARITY_TABLE:&[(&str,&[(&str,u32)])]=&[("x",&[("method",2),("other",0)])];')
        self.assertEqual({k:v[0]for k,v in rows.items()},{("x","method"):"2",("x","other"):"0"})
        overrides=arity_overrides('fn bound_native_callable_value_arity(v:f64)->Option<u32>{match (module,method){("x","method")=>Some(2),_=>None}}')
        self.assertEqual({k:v[0]for k,v in overrides.items()},{("x","method"):"2"})
if __name__=="__main__":unittest.main()
