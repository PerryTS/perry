//! A top-level function declaration stays a direct `FuncRef` callee only
//! while nothing stores into its binding; a store from a nested function
//! makes it a mutable module binding read at every call.

fn lower(source: &str) -> (crate::ir::Module, String) {
    let parsed = perry_parser::parse_typescript(source, "t.ts").unwrap();
    let hir = super::lower_module(&parsed, "t", "t.ts").unwrap();
    let init = format!("{:?}", hir.init);
    (hir, init)
}

fn func_id(hir: &crate::ir::Module, name: &str) -> crate::types::FuncId {
    hir.functions.iter().find(|f| f.name == name).unwrap().id
}

#[test]
fn never_stored_declaration_is_a_direct_callee() {
    let (hir, init) = lower(
        r#"
        function pick(c: any) { return c ? "a" : "b"; }
        function other(pick: any) { pick = 1; let q = 0; q = 2; return q; }
        function third() { let pick: any = 0; pick = 1; return pick; }
        console.log(pick(true), other(0), third());
    "#,
    );
    let id = func_id(&hir, "pick");
    assert!(
        init.contains(&format!("callee: FuncRef({id})")),
        "a declaration stored into only through shadowing locals must stay a direct callee: {init}"
    );
}

#[test]
fn declaration_stored_from_a_nested_function_is_a_binding() {
    for store in [
        "function rebind() { pick = function (c: any) { return k + String(c); }; }",
        "const rebind = () => { (pick as any) = () => k; };",
        "class R { m() { (pick as any) = () => k; } }",
        "function rebind() { (pick as any) += k; }",
        "function rebind() { [pick] = [() => k] as any; }",
        "function rebind() { ({ pick } = { pick: () => k } as any); }",
        "function rebind() { for (pick of [() => k] as any); }",
    ] {
        let source = format!(
            r#"
            const k = "y";
            function pick(c: any) {{ return c ? "a" : "b"; }}
            {store}
            console.log(pick(true));
        "#
        );
        let (hir, init) = lower(&source);
        let id = func_id(&hir, "pick");
        assert!(
            !init.contains(&format!("callee: FuncRef({id})")),
            "`{store}` stores into `pick`; a call must read the binding: {init}"
        );
        assert!(
            init.contains(&format!("init: Some(FuncRef({id}))")),
            "`{store}`: the binding must be seeded with the declaration: {init}"
        );
    }
}
