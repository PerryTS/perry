use crate::lower::LoweringContext;
use crate::lower_decl::lower_block_stmt;

fn lower(source: &str) -> crate::Module {
    let parsed = perry_parser::parse_typescript(source, "interface.ts").unwrap();
    crate::lower_module(&parsed, "test", "interface.ts").unwrap()
}

#[test]
fn body_interfaces_keep_their_own_array_named_methods() {
    for generic in [false, true] {
        let declaration = if generic {
            "interface Stack<T> { push(x: T): number; size(): number }"
        } else {
            "interface Stack { push(x: number): number; size(): number }"
        };
        let ty = if generic { "Stack<number>" } else { "Stack" };
        for forward in [false, true] {
            let call = format!("const s: {ty} = make(); s.push(1);");
            let body = if forward {
                format!("{call} {declaration}")
            } else {
                format!("{declaration} {call}")
            };
            for source in [
                format!("function f(make: any) {{ {body} }}"),
                format!("const f = (make: any) => {{ {body} }};"),
                format!("const f = function(make: any) {{ {body} }};"),
                format!("async function f(make: any) {{ {body} }}"),
                format!("function* f(make: any) {{ {body} }}"),
                format!("class C {{ f(make: any) {{ {body} }} }}"),
                format!("function f(make: any) {{ for (let i = 0; i < 1; i++) {{ {body} }} }}"),
                format!("function f(make: any) {{ {{ {body} }} }}"),
                format!("function f(make: any) {{ if (true) {{ {body} }} }}"),
                format!("function f(make: any) {{ try {{ {body} }} finally {{}} }}"),
                format!("function f(make: any) {{ switch (1) {{ case 1: {body} }} }}"),
                format!("const make: any = () => ({{}}); {{ {body} }}"),
                format!("const make: any = () => ({{}}); switch (1) {{ case 1: {body} }}"),
            ] {
                let module = lower(&source);
                let hir = format!("{module:?}");
                assert!(
                    !hir.contains("ArrayPush"),
                    "user push became ArrayPush: {source}\n{hir}"
                );
                assert!(
                    hir.contains("PropertyGet") && hir.contains("push"),
                    "own method dispatch missing: {hir}"
                );
            }
        }
    }
}

#[test]
fn forward_interfaces_are_visible_to_nested_function_and_arrow_bodies() {
    let module = lower(
        r#"
        function f(make: any) {
            function nested() { const s: Stack = make(); s.push(1); }
            const arrow = () => { const s: Stack = make(); s.push(2); };
            interface Stack { push(x: number): void }
            nested(); arrow();
        }
    "#,
    );
    assert!(!format!("{module:?}").contains("ArrayPush"));
}

#[test]
fn real_arrays_still_lower_to_array_push() {
    let module = lower(
        r#"
        function f() {
            interface Stack { push(x: number): void }
            const numbers: number[] = [];
            numbers.push(1);
        }
    "#,
    );
    assert!(format!("{module:?}").contains("ArrayPush"));
}

#[test]
fn block_interface_metadata_does_not_escape_its_scope() {
    let parsed = perry_parser::parse_typescript(
        "{ interface Item { inner: number } interface Local { value: string } }",
        "interface.ts",
    )
    .unwrap();
    let swc_ecma_ast::ModuleItem::Stmt(swc_ecma_ast::Stmt::Block(block)) = &parsed.body[0] else {
        panic!("expected block");
    };
    let mut ctx = LoweringContext::new("interface.ts");
    ctx.interfaces.push(("Item".into(), 999));
    ctx.interface_source_keys
        .insert("Item".into(), vec!["outer".into()]);
    lower_block_stmt(&mut ctx, block).unwrap();
    assert_eq!(ctx.interfaces, vec![("Item".into(), 999)]);
    assert_eq!(ctx.interface_source_keys["Item"], ["outer"]);
    assert!(!ctx.interface_source_keys.contains_key("Local"));
    assert!(!ctx.interface_object_types.contains_key("Item"));
    assert!(!ctx.interface_object_types.contains_key("Local"));
}

#[test]
fn interface_asserted_expressions_keep_their_own_push() {
    for (declaration, ty) in [
        ("interface Stack { push(x: number): number }", "Stack"),
        ("interface Stack<T> { push(x: T): number }", "Stack<number>"),
    ] {
        let source = format!("function f(make: any) {{ {declaration} (make() as {ty}).push(1); }}");
        let module = lower(&source);
        assert!(!format!("{module:?}").contains("ArrayPush"), "{source}");
    }
}
