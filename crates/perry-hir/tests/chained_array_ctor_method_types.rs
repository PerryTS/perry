//! `new Array<T>(n)` types as `Generic { base: "Array", type_args: [T] }`
//! (the explicit-type-args early return in `infer_type_from_expr`), while the
//! builtin array method-return table matched only `Type::Array`. A chained
//! receiver-returning method — `new Array<number>(n).fill(0)`, the canonical
//! preallocation idiom — therefore inferred `Any`, the binding lost its array
//! type, and every later `a[i] = v` on it took the generic feedback store
//! instead of the inline guarded lane: 34.6 vs 3.7 ns per store, for the
//! array's whole life. The rebind in `lower_types.rs` normalizes the Generic
//! spelling to `Type::Array` before the method tables; these tests pin it.

use perry_diagnostics::SourceCache;
use perry_hir::types::Type;
use perry_hir::{lower_module, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn find_local_type<'m>(module: &'m Module, name: &str) -> &'m Type {
    for s in &module.init {
        if let Stmt::Let { name: n, ty, .. } = s {
            if n == name {
                return ty;
            }
        }
    }
    panic!("binding {name} not found in module.init");
}

#[test]
fn chained_fill_keeps_number_array_type() {
    let module = lower_src("const a = new Array<number>(8192).fill(0);");
    assert_eq!(
        find_local_type(&module, "a"),
        &Type::Array(Box::new(Type::Number)),
        "new Array<number>(n).fill(0) must infer number[], not Any"
    );
}

#[test]
fn double_chain_through_receiver_returning_methods() {
    let module = lower_src("const b = new Array<number>(4).fill(1).reverse();");
    assert_eq!(
        find_local_type(&module, "b"),
        &Type::Array(Box::new(Type::Number)),
        "receiver-returning chains off new Array<T> must keep T[]"
    );
}

#[test]
fn element_returning_method_sees_element_type() {
    let module = lower_src("const c = new Array<number>(4).fill(2).at(0);");
    assert_eq!(
        find_local_type(&module, "c"),
        &Type::Number,
        ".at() on a new Array<number> chain must return the element type"
    );
}
