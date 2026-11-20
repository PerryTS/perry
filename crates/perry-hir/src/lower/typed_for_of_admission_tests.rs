//! A for-of over a source annotated as a numeric typed array is walked by
//! index only under the runtime admission `typedArrayForOfAdmits`; the other
//! arm iterates the same value by its own iterator. Heads the index rewrite
//! does not take drain any other value with `ForOfToArray`.

#![cfg(test)]

use perry_diagnostics::SourceCache;

fn lowered(src: &str) -> String {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "ta_for_of.ts", &mut cache)
                    .expect("parse should succeed");
            let module = crate::lower_module(&parsed.module, "test", "ta_for_of.ts")
                .expect("lower should succeed");
            format!("{module:?}")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn function_body_loop_is_admitted() {
    let hir =
        lowered("function f(a: Float64Array) { let s = 0; for (const x of a) s += x; return s; }");
    assert!(hir.contains("typedArrayForOfAdmits"), "{hir}");
    // The other arm is the IteratorRecord loop over an `any` copy.
    assert!(hir.contains("__forof_lit_any_"), "{hir}");
}

#[test]
fn module_loop_is_admitted() {
    let hir =
        lowered("const a: Uint8Array = new Uint8Array(2); for (const x of a) console.log(x);");
    assert!(hir.contains("typedArrayForOfAdmits"), "{hir}");
}

#[test]
fn pattern_head_drains_other_values() {
    let hir = lowered("function f(a: Int32Array) { for (const [x] of a) console.log(x); }");
    assert!(hir.contains("typedArrayForOfAdmits"), "{hir}");
    assert!(hir.contains("ForOfToArray"), "{hir}");
}

#[test]
fn plain_array_source_keeps_its_proof() {
    let hir =
        lowered("function f(a: number[]) { let s = 0; for (const x of a) s += x; return s; }");
    assert!(!hir.contains("typedArrayForOfAdmits"), "{hir}");
    assert!(hir.contains("arrayRecordNeedsIterator"), "{hir}");
}
