//! Successful result summaries for admitted native HIR operations.
use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    /// The realm constructor value a receiver-less `NativeMethodCall` is
    /// `new` of (`perry_hir::folded_call::native_construct_fold`): the
    /// function value the realm declares as `method` of node module
    /// `module` (`node:inspector.Session`). `None`: not such a fold, or a
    /// realm that declares no such function.
    pub(crate) fn native_construct_fold(&mut self, module: &str, method: &str) -> Option<Tok> {
        if self.m.unit_mode || !perry_hir::folded_call::native_construct_fold(module, method) {
            return None;
        }
        let ns = crate::builtins::node_module_ns(module)?;
        let identity = self.m.namespace_property(ns, method)?.native?;
        Some(self.m.tok(TokKind::BuiltinFn(Native::Member {
            identity,
            holder: None,
        })))
    }

    /// The receiver and callee of a receiver-less `NativeMethodCall` the HIR
    /// folds from a method call on a realm global
    /// (`perry_hir::folded_call::native_global_method_fold`:
    /// `performance.mark(name)`): the realm's own global object and the
    /// function the realm declares as `method` on its chain
    /// (`Performance.prototype.mark`). `None`: not such a fold, or a realm
    /// that declares no such function.
    pub(crate) fn native_global_method(&mut self, module: &str, method: &str) -> Option<(Tok, Tok)> {
        let global = perry_hir::folded_call::native_global_method_fold(module, method)?;
        let ns = crate::builtins::ns_by_name(global)?;
        let identity = self.m.namespace_property(ns, method)?.native?;
        let recv = self.m.tok(TokKind::Builtin(ns));
        let f = self.m.tok(TokKind::BuiltinFn(Native::Member {
            identity,
            holder: None,
        }));
        Some((recv, f))
    }

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
