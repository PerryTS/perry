//! WTC9 (F3a): wasm32 lowers `recv.m(args)` and `obj[key]` through the same
//! object-model sites as LP64 (the method site's builtin entry, the keyed
//! probe with its site memo), not the generic runtime front.

const SRC: &str = r#"
export function bump(m: any, k: any): any {
    m.set(k, (m.get(k) || 0) + 1);
    return m;
}
export function pick(d: any, keys: any, i: number): any {
    return d[keys[i & 3]];
}
"#;

fn compile(target: &str) -> String {
    let ast = perry_parser::parse_typescript(SRC, "wasm32_sites.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "wasm32_sites", "wasm32_sites.ts").unwrap();
    String::from_utf8(
        crate::compile_module(
            &hir,
            crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: true,
                target: Some(target.into()),
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn calls(ir: &str, callee: &str) -> bool {
    ir.lines()
        .any(|l| !l.trim_start().starts_with("declare") && l.contains(&format!("@{callee}(")))
}

#[test]
fn wasm32_method_and_keyed_sites_match_lp64() {
    let native = compile("x86_64-unknown-linux-gnu");
    let wasm = compile("wasm32-wasip2");
    for callee in [
        "js_bm_get1",
        "js_bm_set2",
        "perry_probe_get_keyed",
        "js_object_get_keyed_site_slow",
    ] {
        assert!(calls(&native, callee), "native lost {callee}");
        assert!(calls(&wasm, callee), "wasm32 does not use {callee}");
    }
    for generic in [
        "js_typed_feedback_native_call_method",
        "js_typed_feedback_native_call_method_by_id",
    ] {
        assert!(!calls(&wasm, generic), "wasm32 still calls {generic}");
    }
    assert!(wasm.contains("_read_shape_slots = private global [24 x i64] zeroinitializer, align 8"));
    // The adapted module is valid wasm32 code.
    let object = crate::linker::compile_ll_to_object(&wasm, Some("wasm32-wasip2")).unwrap();
    assert!(object.starts_with(b"\0asm"));
}
