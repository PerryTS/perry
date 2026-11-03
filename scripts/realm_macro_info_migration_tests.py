import unittest
from realm_macro_info_migration import expose
class MacroInfoMigrationTests(unittest.TestCase):
    def test_abi_moves_to_calls_without_line_shifts(self):
        source='''fn install_domain_prototype(proto: &mut PayloadPrototype) {
    macro_rules! method {
        ($name:literal, $body:ident, $n:tt) => {
            proto.method($name, perry_runtime::fn_info!(
                $body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN)
            ), $n)
        };
    }
'''+''.join(f'method!("m{n}", domain_m{n}_thunk, {n});\n'for n in range(7))+'}\n'
        changed=expose(source)
        self.assertEqual(source.count('\n'),changed.count('\n'))
        self.assertEqual(changed.count('perry_runtime::fn_info!('),7)
        self.assertIn('domain_m2_thunk, 2; with_declared(2), with_flags(perry_runtime::closure::FN_BUILTIN)',changed)
        self.assertNotIn('$body',changed)
