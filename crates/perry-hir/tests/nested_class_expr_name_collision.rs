//! Batch-3 functional-correctness fix (minimatch source-compile): a NAMED
//! class EXPRESSION nested in a function body whose name collides with a
//! TOP-LEVEL class DECLARATION in the same module must NOT reuse the
//! declaration's ClassId / module-scope registration.
//!
//! minimatch's `defaults()` returns
//!   `Object.assign(m, { Minimatch: class Minimatch extends orig.Minimatch
//!      { constructor(){ super(...) } static defaults(){…} } })`
//! — a near-empty nested class expression that happens to share the name of
//! the real top-level `export class Minimatch { …18 fields, ~10 methods… }`.
//! Per JS spec a class-expression's name binds only inside its own body, so
//! the two are distinct classes. Perry reused the top-level class's ClassId
//! for the nested expression, silently overwriting the real exported class
//! with the body-less nested one — `new Minimatch(pattern)` then produced an
//! instance whose every field/method was `undefined`.
//!
//! The fix renames the colliding class expression to a fresh unique name so
//! it gets its own ClassId; the value position (object property / `new` site)
//! holds the resulting ClassRef directly.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                parse_typescript_with_cache(&src, "/tmp/nested_class_collision.ts", &mut cache)
                    .expect("parse failed");
            lower_module(&parsed.module, "test", "/tmp/nested_class_collision.ts")
                .expect("lower failed")
        })
        .expect("spawn")
        .join()
        .expect("lower thread panicked")
}

/// Find the lowered `Class` whose name is exactly `name` (not a synthetic
/// `<name>__class_expr_N` rename).
fn find_class<'a>(module: &'a Module, name: &str) -> Option<&'a perry_hir::Class> {
    module.classes.iter().find(|c| c.name == name)
}

#[test]
fn nested_class_expr_does_not_overwrite_toplevel_declaration() {
    // The nested `class Thing` appears in file order BEFORE the real
    // top-level `class Thing` — exactly the minimatch shape (the nested
    // expression in `defaults()` precedes `export class Minimatch`).
    let src = r#"
        const reg: any = (p: any) => p;
        export const defaults = (def: any) => {
            const orig: any = reg;
            const m: any = (p: any) => orig(p);
            return Object.assign(m, {
                Thing: class Thing extends orig.Thing {
                    constructor(x: any) { super(x); }
                    static defaults() { return null; }
                },
            });
        };
        export class Thing {
            a;
            set;
            constructor(x: any) {
                this.a = x;
                this.set = [1, 2];
            }
            greet() { return "hi"; }
            make() { return this.set.length; }
        }
    "#;
    let module = lower(src);

    // The real top-level `Thing` must survive with its full body — at least
    // one real method (`greet`/`make`) and its declared fields. The nested
    // expression (constructor + a single static) must NOT have clobbered it.
    let thing = find_class(&module, "Thing").expect("top-level `Thing` class must exist");
    let method_names: Vec<&str> = thing.methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        method_names.contains(&"greet") && method_names.contains(&"make"),
        "top-level `Thing` must keep its real methods (got methods: {:?})",
        method_names
    );
    assert!(
        thing.fields.iter().any(|f| f.name == "a") && thing.fields.iter().any(|f| f.name == "set"),
        "top-level `Thing` must keep its declared fields"
    );
}
