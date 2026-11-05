//! #13660: rejected fast clones must not leave executable admission work.

fn compile(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "packed_admission.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "packed_admission", "packed_admission.ts").unwrap();
    String::from_utf8(
        crate::compile_module(
            &hir,
            crate::CompileOptions {
                emit_ir_only: true,
                target: Some("aarch64-apple-darwin".into()),
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn simplify(ir: &str) -> String {
    // Only remove unreachable CFG and dead instructions: no inlining or
    // runtime knowledge may hide an otherwise executable admission call.
    crate::inprocess::statepoint_rewritten_ir_with_passes(
        ir,
        "aarch64-apple-darwin",
        "packed_admission",
        "function(simplifycfg,dce)",
    )
    .unwrap()
}

fn probe_ir(body: &str) -> String {
    compile(&format!(
        r#"
        export function probe(steps: number[], cells: number[], origin: number): boolean {{
            for (let j = 0; j < steps.length; j++) {{ {body} }}
            return false;
        }}
        "#
    ))
}

#[test]
fn rejected_clone_has_no_live_admission_or_descriptor() {
    let ir = probe_ir(
        r#"
        const square = origin + (steps[j] ?? 0);
        if (square >= 0 && square < 128 && (square & 0x88) === 0 && (cells[square] ?? 0) === 2)
            return true;
        "#,
    );
    assert!(
        ir.contains("for.stable_packed_fast"),
        "fixture must attempt a fast clone: {ir}"
    );
    let ir = simplify(&ir);
    assert!(
        !ir.contains("call i64 @js_packed_arraylike_loop_guard_live("),
        "rejected clone left admission live: {ir}"
    );
    assert!(
        !ir.contains("alloca [7 x i64]"),
        "rejected clone left an unused descriptor: {ir}"
    );
    assert!(
        !ir.contains("for.stable_packed_fast"),
        "rejected clone survived: {ir}"
    );
    assert!(
        ir.contains("for.stable_packed_slow"),
        "fallback must survive: {ir}"
    );
}

#[test]
fn usable_clone_keeps_admission_and_descriptor() {
    // An erased receiver exercises stable-packed admission rather than the
    // earlier typed packed-f64 tier.
    let ir = compile(
        r#"
        export function probe(steps: any): boolean {
            for (let j = 0; j < steps.length; j++) {
                if ((steps[j] | 0) === 2) return true;
            }
            return false;
        }
    "#,
    );
    let ir = simplify(&ir);
    assert!(
        ir.contains("call i64 @js_packed_arraylike_loop_guard_live("),
        "usable clone lost admission: {ir}"
    );
    assert!(
        ir.contains("alloca [7 x i64]"),
        "usable clone lost its descriptor: {ir}"
    );
    assert!(
        ir.contains("for.stable_packed_fast"),
        "usable fast clone must survive: {ir}"
    );
    assert!(
        ir.contains("for.stable_packed_slow"),
        "admission failure must retain fallback: {ir}"
    );
}
