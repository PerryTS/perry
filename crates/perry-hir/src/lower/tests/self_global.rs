fn lower(source: &str) -> crate::ir::Module {
    let ast = perry_parser::parse_typescript(source, "self-global.ts").unwrap();
    super::super::lower_module(&ast, "self-global", "self-global.ts").unwrap()
}

#[test]
fn self_global_member_reads_resolve_the_actual_binding() {
    let hir = lower("const value = self.Math.max(1, 4); const direct = self;");
    let dump = format!("{:#?}", hir.init);
    assert!(dump.contains("js_global_get_or_throw_unresolved"), "{dump}");
    assert!(
        !dump.contains("MathMax"),
        "self.Math must not become an intrinsic: {dump}"
    );
}

#[test]
fn self_global_typeof_uses_optional_lookup() {
    let hir = lower("const type = typeof self;");
    let dump = format!("{:#?}", hir.init);
    assert!(dump.contains("js_global_get_optional"), "{dump}");
    assert!(
        !dump.contains("js_global_get_or_throw_unresolved"),
        "{dump}"
    );
}

#[test]
fn self_global_lexical_shadowing_keeps_the_local() {
    let hir = lower("function f(self: any) { return self.Math.max(1, 4); }");
    let dump = format!("{:#?}", hir.functions);
    assert!(dump.contains("LocalGet"), "{dump}");
    assert!(
        !dump.contains("js_global_get_or_throw_unresolved"),
        "{dump}"
    );
}
