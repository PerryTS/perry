//! A value read of the `gc` global (`const g = gc; g()`) is the
//! `globalThis.gc` function, not the `true` an extern function reference
//! lowered to. A call `gc()` keeps its direct form.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lowered(src: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed =
        parse_typescript_with_cache(src, "gc_global_value.js", &mut cache).expect("fixture parses");
    let module =
        lower_module(&parsed.module, "test", "gc_global_value.js").expect("fixture lowers");
    format!("{module:#?}")
}

#[test]
fn value_read_of_gc_is_not_an_extern_function_ref() {
    let dump = lowered("const g = gc; console.log(typeof g);");
    assert!(
        !dump.contains("ExternFuncRef"),
        "a value read of `gc` must not lower to an extern function reference:\n{dump}"
    );
}
