import unittest
from realm_table_length_migration import table_lengths

class TableLengthMigrationTests(unittest.TestCase):
    def test_only_tables_consumed_by_the_birth_are_selected(self):
        source='const USED:&[(&str,u32)]=&[("a",0),("b",2)]; const UNUSED:&[(&str,u32)]=&[("wrong",3)]; install_noop_proto_methods(p,USED); install_noop_proto_methods(p,&[("c",1)]);'
        self.assertEqual([(k,t[0])for k,t in table_lengths(source)],[("a","0"),("b","2"),("c","1")])
        self.assertEqual(table_lengths('fn install_noop_proto_methods(p: P, methods: &[(&str,u32)]) {}'),[])
if __name__=="__main__":unittest.main()
