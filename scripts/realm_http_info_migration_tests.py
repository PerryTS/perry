import unittest
from unittest.mock import patch
from realm_http_info_migration import rewrite
class HttpInfoMigrationTests(unittest.TestCase):
    def test_static_identity_and_body_stay_in_the_existing_macro(self):
        source='macro_rules! method {\n ($body:ident, $op:ident, $info:ident, $min:expr, $server_only:expr) => {\n static $info: JsFunctionInfo = JsFunctionInfo::of($body as JsBody3).with_declared(3);\n }; }\nmethod!(write_body, write, WRITE, 1, false);\n'
        d={'name':'Example.write','body':'example::write_body'}
        with patch('realm_source_migration.simple_facts',return_value={}):
            changed,placements=rewrite(source,[d])
        self.assertIn('static $info: JsFunctionInfo = $initializer;',changed)
        self.assertIn('method!(write_body, write, WRITE, 1, false, perry_ffi::realm_decl!',changed)
        self.assertEqual(placements[0]['line'],5)
        self.assertEqual(changed.count('\n'),source.count('\n'))
