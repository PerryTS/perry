import unittest
from realm_info_expression_migration import expressions
import realm_source_migration as m

class InfoExpressionTests(unittest.TestCase):
    def test_typed_table_row_keeps_builders_and_body(self):
        m.CONSTANTS={'FN_HAS_DECLARED':2048,'FN_REST_MASK':65543}
        s='ExportSpec { name: "x", info: JsFunctionInfo::of(body as JsBody1<ClosureHeader>).with_declared(2), }'
        found=expressions(s)
        self.assertEqual(len(found),1)
        self.assertEqual(found[0]['body'],'body')
        self.assertEqual(found[0]['signature']['declared'],2)
        self.assertEqual(s[found[0]['offset']:found[0]['end']],'JsFunctionInfo::of(body as JsBody1<ClosureHeader>).with_declared(2)')
        self.assertEqual(expressions('realm_decl! { info_expr JsFunctionInfo::of(body as JsBody1<ClosureHeader>); realm { A; } }'),[])

    def test_templates_and_owned_items_have_one_owner(self):
        m.CONSTANTS={}
        self.assertEqual(expressions('macro_rules! make { () => { JsFunctionInfo::of(body as JsBody1<ClosureHeader>) }; }'),[])
        self.assertEqual(expressions('realm_decl! { info static X: JsFunctionInfo = JsFunctionInfo::of(body as JsBody1<ClosureHeader>); realm { X; } }'),[])
