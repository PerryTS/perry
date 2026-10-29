import unittest
from realm_fetch_getter_migration import expose
class FetchGetterMigrationTests(unittest.TestCase):
    def test_infos_move_to_rows_without_line_changes(self):
        src='''macro_rules! fetch_getter_bodies {
($($body:ident => $name:literal),+ $(,)?) => {
$($name => crate::fn_info!($body, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),)+
}; }
fetch_getter_bodies! {
body => "body",
}'''
        result=expose(src)
        self.assertEqual(src.count('\n'),result.count('\n'))
        self.assertIn('$($name => $info,)+',result)
        self.assertIn('body => "body", crate::fn_info!(body, 0;',result)
