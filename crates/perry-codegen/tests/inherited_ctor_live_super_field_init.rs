//! #11120: `new F()` of a constructor-free class over a constructor-owning
//! parent inlines the parent's body. When that body's `super()` lowers live
//! (`requires_live`), the live lowering installs every class above the body's
//! class itself: the checked arm stages them before its static parent body,
//! the runtime arm runs the parent constructor. Staging the root's fields
//! before the inlined body as well ran them twice, and a `#private` field threw
//! "Cannot initialize #b twice on the same object".
//!
//! An IR census: in `main`, the root's private field may be installed only
//! inside the `super()` lowering, never before the inlined body enters it.

use perry_codegen::{assign_static_shape_ids, compile_module, module_birth_shapes, CompileOptions};
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

const FIELD_ADD: &str = "@js_private_field_add(";
const SUPER_SCOPE: &str = "@js_derived_super_scope_push(";
const SUPER_PARENT: &str = "@js_class_super_parent(";

fn ir_of(source: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "fieldinit_live.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "fieldinit_live", "fieldinit_live.ts").unwrap();
    let mut options = CompileOptions {
        is_entry_module: true,
        emit_ir_only: true,
        output_type: "executable".into(),
        disable_constfn_shapes: false,
        ..Default::default()
    };
    let births = module_birth_shapes(&module, options.clone()).unwrap();
    options.static_shape_ids = assign_static_shape_ids(births.iter().map(|b| &b.shape))
        .into_iter()
        .collect();
    String::from_utf8(compile_module(&module, options).unwrap()).unwrap()
}

fn main_body(ir: &str) -> &str {
    let start = ir
        .find("define i32 @main(")
        .expect("entry module defines main");
    let body = &ir[start..];
    &body[..body.find("\n}\n").expect("main is closed")]
}

/// The private field adds `main` emits before the inlined body's `super()`
/// begins, and the adds in all of `main`.
fn adds_before_super(source: &str) -> (usize, usize) {
    let ir = ir_of(source);
    let main = main_body(&ir);
    let enter = main
        .find(SUPER_SCOPE)
        .or_else(|| main.find(SUPER_PARENT))
        .unwrap_or_else(|| panic!("the inlined body's super() must lower live in main:\n{main}"));
    assert!(
        main.contains(SUPER_PARENT),
        "the inlined body's super() must reach the parent constructor at run time \
         (the shape under test):\n{main}"
    );
    (
        main[..enter].matches(FIELD_ADD).count(),
        main.matches(FIELD_ADD).count(),
    )
}

#[test]
fn inherited_body_with_live_super_stages_no_root_field() {
    let (before, total) = adds_before_super(
        r#"
class S { #b = 1; get b() { return this.#b; } }
class E extends S { constructor() { super(); } }
class F extends E {}
console.log(new F().b);
"#,
    );
    assert_eq!(
        before, 0,
        "`new F()` staged S's `#b` before E's inlined body although E's super() \
         installs it (total adds in main: {total})"
    );
    assert!(total >= 1, "the checked arm of super() still installs `#b`");
}

/// The same with constructor-free classes between the body's class and the
/// leaf (the gap test's Tail/Between shape): the intermediates are staged after
/// the body, the root only inside its super().
#[test]
fn inherited_body_through_intermediates_stages_no_root_field() {
    let (before, _) = adds_before_super(
        r#"
class Root { #count = 0; get count() { return this.#count; } }
class Mid extends Root { #m = "m"; constructor() { super(); } get m() { return this.#m; } }
class Between extends Mid { between = 1; }
class Tail extends Between { tail = 2; }
const t = new Tail();
console.log(t.count, t.m, t.between, t.tail);
"#,
    );
    assert_eq!(
        before, 0,
        "Root's `#count` was staged before Mid's inlined body"
    );
}
