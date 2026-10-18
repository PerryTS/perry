//! Mark source regions across the CommonJS wrap and static-require rewrite.
//! Markers are comments chosen absent from the input, never runtime state.
pub(in crate::commands::compile) fn markers(source: &str) -> (String, String) {
    let mut n = 0;
    loop {
        let base = format!("__perry_source_region_{n}_");
        if !source.contains(&base) {
            return (format!("/*{base}begin__*/"), format!("/*{base}end__*/"));
        }
        n += 1;
    }
}
pub(in crate::commands::compile) fn mark(text: &str, markers: &(String, String)) -> String {
    if text.is_empty() {
        return String::new();
    }
    format!("{}{}{}", markers.0, text, markers.1)
}
pub(in crate::commands::compile) fn ranges(
    text: &str,
    markers: &(String, String),
) -> Option<Vec<(u32, u32)>> {
    let mut result = Vec::new();
    let mut pos = 0;
    while let Some(start) = text[pos..].find(&markers.0) {
        let start = pos + start + markers.0.len();
        let end = start + text[start..].find(&markers.1)?;
        result.push((u32::try_from(start).ok()? + 1, u32::try_from(end).ok()? + 1));
        pos = end + markers.1.len();
    }
    Some(result)
}

/// Stamp only call/member sites in a private AST copy. Source produced later
/// by constant eval/Function folding is never mistaken for wrapper source.
pub(in crate::commands::compile) fn stamp(
    module: &swc_ecma_ast::Module,
    syntax: &Option<perry_hir::ir::ClassFamilySyntax>,
) -> Option<swc_ecma_ast::Module> {
    use swc_ecma_visit::{VisitMut, VisitMutWith};
    let ranges = syntax.as_ref()?.original_source_ranges.as_ref()?;
    struct Stamp<'a>(&'a [(u32, u32)]);
    impl Stamp<'_> {
        fn mark(&self, span: &mut swc_common::Span) {
            if span.lo.0 != 0
                && !self
                    .0
                    .iter()
                    .any(|&(lo, hi)| lo <= span.lo.0 && span.lo.0 < hi)
            {
                span.lo.0 |= perry_hir::SOURCE_COMPILER_OWNED;
                span.hi.0 |= perry_hir::SOURCE_COMPILER_OWNED;
            }
        }
    }
    impl Stamp<'_> {
        fn function(&self, span: &mut swc_common::Span) {
            let lo = span.lo.0;
            let hi = span.hi.0;
            self.mark(span);
            if span.lo.0 & perry_hir::SOURCE_COMPILER_OWNED != 0
                && !self.0.iter().any(|&(a, b)| lo < b && a < hi)
            {
                span.lo.0 |= perry_hir::SOURCE_COMPILER_BODY;
                span.hi.0 |= perry_hir::SOURCE_COMPILER_BODY;
            }
        }
    }
    impl VisitMut for Stamp<'_> {
        fn visit_mut_ident(&mut self, node: &mut swc_ecma_ast::Ident) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_bin_expr(&mut self, node: &mut swc_ecma_ast::BinExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_unary_expr(&mut self, node: &mut swc_ecma_ast::UnaryExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_cond_expr(&mut self, node: &mut swc_ecma_ast::CondExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_seq_expr(&mut self, node: &mut swc_ecma_ast::SeqExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_paren_expr(&mut self, node: &mut swc_ecma_ast::ParenExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }

        fn visit_mut_import_named_specifier(
            &mut self,
            node: &mut swc_ecma_ast::ImportNamedSpecifier,
        ) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.local.span);
        }
        fn visit_mut_function(&mut self, node: &mut swc_ecma_ast::Function) {
            node.visit_mut_children_with(self);
            self.function(&mut node.span);
        }
        fn visit_mut_arrow_expr(&mut self, node: &mut swc_ecma_ast::ArrowExpr) {
            node.visit_mut_children_with(self);
            self.function(&mut node.span);
        }
        fn visit_mut_var_declarator(&mut self, node: &mut swc_ecma_ast::VarDeclarator) {
            node.visit_mut_children_with(self);
            if let Some(value) = &mut node.init {
                use swc_ecma_ast::Expr as A;
                match value.as_mut() {
                    A::Ident(x) => self.mark(&mut x.span),
                    A::Object(x) => self.mark(&mut x.span),
                    A::Array(x) => self.mark(&mut x.span),
                    A::Bin(x) => self.mark(&mut x.span),
                    A::Paren(x) => self.mark(&mut x.span),
                    _ => {}
                }
            }
        }
        fn visit_mut_call_expr(&mut self, node: &mut swc_ecma_ast::CallExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_member_expr(&mut self, node: &mut swc_ecma_ast::MemberExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_assign_expr(&mut self, node: &mut swc_ecma_ast::AssignExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
        fn visit_mut_update_expr(&mut self, node: &mut swc_ecma_ast::UpdateExpr) {
            node.visit_mut_children_with(self);
            self.mark(&mut node.span);
        }
    }
    let mut owned = module.clone();
    owned.visit_mut_with(&mut Stamp(ranges));
    Some(owned)
}
#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{Expr, SOURCE_COMPILER_OWNED};

    fn wrapped(body: &str) -> (swc_ecma_ast::Module, perry_hir::ir::ClassFamilySyntax) {
        let markers = markers(body);
        let text=format!("import {{createRequire as __perry_cjs_create_require}} from 'node:module';\nconst internal = __perry_cjs_create_require('/tmp/ro.cjs');\n{}",mark(body,&markers));
        let syntax = perry_hir::ir::ClassFamilySyntax {
            original_source_ranges: ranges(&text, &markers),
            ..Default::default()
        };
        let ast = perry_parser::parse_typescript(&text, "ro.cjs").unwrap();
        (stamp(&ast, &Some(syntax.clone())).unwrap(), syntax)
    }
    fn facts(body: &str) -> perry_wpfacts::ProgramFacts {
        let (ast, syntax) = wrapped(body);
        let mut hir = perry_hir::lower_module(&ast, "ro", "ro.cjs").unwrap();
        hir.set_class_family_syntax(Some(syntax));
        perry_wpfacts::compute(&[("ro.cjs".into(), &hir)], &Default::default())
    }
    #[test]
    fn comments_are_fresh_and_survive_a_prefix_rewrite() {
        let body = "console.log('__perry_source_region_0_begin__');";
        let markers = markers(body);
        assert!(!body.contains(&markers.0));
        let text = format!("import x from 'x';\n{}", mark(body, &markers));
        let rs = ranges(&text, &markers).unwrap();
        assert_eq!(rs.len(), 1);
        assert_eq!(&text[rs[0].0 as usize - 1..rs[0].1 as usize - 1], body);
    }
    #[test]
    fn identical_user_and_compiler_loader_calls_keep_distinct_provenance() {
        let (ast, _) = wrapped(
            "function user() { return __perry_cjs_create_require('/tmp/ro.cjs'); } user();",
        );
        let hir = perry_hir::lower_module(&ast, "ro", "ro.cjs").unwrap();
        let mut internal = 0;
        let mut user = 0;
        perry_hir::for_each_module_expr(&hir, &mut |e| match e {
            Expr::Call {
                callee,
                byte_offset,
                ..
            } if *byte_offset & SOURCE_COMPILER_OWNED != 0
                && matches!(callee.as_ref(),Expr::ExternFuncRef {name,..} if name=="js_module_create_require_devirt") =>
            {
                internal += 1
            }
            Expr::NativeMethodCall { module, method, .. }
                if module == "module" && method == "createRequire" =>
            {
                user += 1
            }
            _ => {}
        });
        assert_eq!((internal, user), (1, 1));
    }
    #[test]
    fn user_loader_body_is_not_trusted_by_its_enclosing_wrapper() {
        assert!(facts(
            "function dead() { return __perry_cjs_create_require('/tmp/ro.cjs'); } console.log(1);"
        )
        .module_reentry
        .is_none());
        assert!(facts(
            "function live() { return __perry_cjs_create_require('/tmp/ro.cjs'); } live();"
        )
        .module_reentry
        .is_some());
    }
    #[test]
    fn folded_dynamic_source_is_user_code_even_when_its_offsets_overlap_the_preamble() {
        let body = "const make = new Function(\"return process.getBuiltinModule('module')\");";
        assert!(facts(&format!("{body} console.log(1);"))
            .module_reentry
            .is_none());
        assert!(facts(&format!("{body} make();")).module_reentry.is_some());
    }
    #[test]
    fn generated_factory_forwarding_is_not_a_user_loader_read() {
        for (body, refused) in [
            ("console.log(1);", false),
            (
                "const exposed=__perry_cjs_factory; console.log(typeof exposed);",
                true,
            ),
        ] {
            let marks = markers(body);
            let text=format!("import {{createRequire as __perry_cjs_create_require}} from 'node:module'; function __perry_cjs_factory() {{ const forwarded=__perry_cjs_factory; if (forwarded !== undefined) {{}} {} }} __perry_cjs_factory();",mark(body,&marks));
            let syntax = perry_hir::ir::ClassFamilySyntax {
                original_source_ranges: ranges(&text, &marks),
                ..Default::default()
            };
            let ast = perry_parser::parse_typescript(&text, "forward.cjs").unwrap();
            let ast = stamp(&ast, &Some(syntax.clone())).unwrap();
            let mut hir = perry_hir::lower_module(&ast, "forward", "forward.cjs").unwrap();
            hir.set_class_family_syntax(Some(syntax));
            let p = perry_wpfacts::compute(&[("forward.cjs".into(), &hir)], &Default::default());
            assert_eq!(
                p.module_reentry.is_some(),
                refused,
                "{body}: {:?}",
                p.module_reentry
            );
        }
    }
    #[test]
    fn shorthand_reads_keep_loader_binding_roles() {
        for body in [
            "const held={module};console.log(typeof held);",
            "const held={module,...{}};console.log(typeof held);",
            "const held={require};console.log(typeof held);",
        ] {
            let marks = markers(body);
            let text=format!("import {{createRequire as __perry_cjs_create_require}} from 'node:module'; function require(x){{return x;}} const module={{__perry_cjs_record:true}}; {}",mark(body,&marks));
            let syntax = perry_hir::ir::ClassFamilySyntax {
                original_source_ranges: ranges(&text, &marks),
                ..Default::default()
            };
            let ast = perry_parser::parse_typescript(&text, "short.cjs").unwrap();
            let ast = stamp(&ast, &Some(syntax.clone())).unwrap();
            let mut module = perry_hir::lower_module(&ast, "short", "short.cjs").unwrap();
            module.set_class_family_syntax(Some(syntax));
            let p = perry_wpfacts::compute(&[("short.cjs".into(), &module)], &Default::default());
            assert!(p.module_reentry.is_some(), "{body}: {:?}", p.module_reentry);
        }
    }
}
