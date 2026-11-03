//! Successful result summaries for admitted native HIR operations.
use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    pub(crate) fn native_success_result(
        &mut self,
        module: &str,
        has_object: bool,
        method: &str,
        args: &[Expr],
    ) -> NodeId {
        // The canonical HIR native binding fixes the operation. Only the
        // synchronous UTF-8 overload returns a primitive String; its path,
        // contents and length remain unknown. Dynamic options, callbacks
        // and promises keep TOP. No TypeScript annotation grants this proof.
        if module.strip_prefix("node:").unwrap_or(module) == "fs"
            && !has_object
            && method == "readFileSync"
            && matches!(args.get(1), Some(Expr::String(enc))
                if enc.eq_ignore_ascii_case("utf8") || enc.eq_ignore_ascii_case("utf-8"))
        {
            self.prim_node(prim::STR)
        } else {
            self.m
                .top_val(&format!("result of native module {module}.{method}"))
        }
    }
}
