//! A literal local that nothing writes keeps its contents proof: its
//! property reads multiply as a bare `fmul double`. The same literal after a
//! `delete` must not, or `o.y * 10` reads the deleted slot (F13). Both
//! literals are handed to a read-only callee, so neither is scalar-replaced
//! and only the binding proof decides the product.

fn compile(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "contents_stable.ts").unwrap();
    let m = perry_hir::lower_module(&ast, "contents_stable", "contents_stable.ts").unwrap();
    String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn body<'a>(ir: &'a str, name: &str) -> &'a str {
    let at = ir
        .find(&format!("@perry_fn_contents_stable__{name}("))
        .and_then(|p| {
            ir[..p]
                .rfind("define ")
                .filter(|d| !ir[*d..p].contains('\n'))
        })
        .unwrap_or_else(|| panic!("no body for {name}: {ir}"));
    let end = ir[at..].find("\n}\n").unwrap() + at;
    &ir[at..end]
}

const SOURCE: &str = r#"
    function peek(o: { x: number; y: number }): number {
        return o.x;
    }
    function stable(): number {
        const o = { x: 1.5, y: 2.5 };
        peek(o);
        return o.x * o.y;
    }
    function deleted(): number {
        const o = { x: 1.5, y: 2.5 };
        peek(o);
        delete (o as any).y;
        return o.x * o.y;
    }
    console.log(stable(), deleted());
"#;

#[test]
fn a_literal_local_nothing_writes_keeps_its_contents_proof() {
    let ir = compile(SOURCE);
    let stable = body(&ir, "stable");
    let deleted = body(&ir, "deleted");
    assert!(
        stable.contains("fmul double") && !stable.contains("@js_dynamic_mul("),
        "a stable literal multiplies its proven slots: {stable}"
    );
    assert!(
        deleted.contains("@js_dynamic_mul("),
        "a deleted property must not keep the number proof: {deleted}"
    );
}
