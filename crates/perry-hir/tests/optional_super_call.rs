//! `super.m?.(args)`: the optional call on a SuperProperty reference calls the
//! member with this = the current `this` (13.3.7.1, 13.3.9.1), like the plain
//! `super.m(args)`.
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lowered(src: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "optional_super_call.js", &mut cache)
        .expect("parse");
    let module = lower_module(&parsed.module, "test", "optional_super_call.js").expect("lower");
    format!("{module:?}")
}

#[test]
fn optional_super_call_calls_with_this() {
    let hir = lowered(
        "class A { f(x) { this.v = x; } }
         class B extends A { f(x) { super.f?.(x); return this.v; } }",
    );
    assert!(hir.contains("ReflectApply"), "{hir}");
    assert!(hir.contains("this_arg: This"), "{hir}");
}

#[test]
fn optional_super_call_in_static_method_calls_with_this() {
    let hir = lowered(
        "class A { static s(x) { return [this, x]; } }
         class B extends A { static s(x) { return super.s?.(x); } }",
    );
    assert!(hir.contains("this_arg: This"), "{hir}");
}

#[test]
fn dynamic_super_key_reads_from_home_prototype() {
    let hir = lowered(
        "const k = 'f';
         class A { f(x) { return x; } }
         class B extends A { f(x) { return 0; } run(x) { return [super[k]?.(x), super[k]]; } }",
    );
    assert!(hir.contains("ObjectSuperPropertyGet"), "{hir}");
    assert!(hir.contains("this_arg: This"), "{hir}");
}
