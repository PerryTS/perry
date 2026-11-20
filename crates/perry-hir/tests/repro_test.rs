// `class A extends B` in the same module as a top-level `const x = fn()`
// call to an unannotated factory lowers without overflowing the stack.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "test.ts", &mut cache).unwrap();
    let _ = lower_module(&parsed.module, "test", "test.ts").unwrap();
}

#[test]
fn class_extends_beside_a_top_level_factory_call_lowers() {
    lower(
        r#"
        class Base { b: number; constructor(b: number) { this.b = b; } }
        class Derived extends Base {
            d: number;
            constructor(b: number, d: number) { super(b); this.d = d; }
        }
        function makeCfg() { return { scale: 1 }; }
        const cfg = makeCfg();
        "#,
    );
}
