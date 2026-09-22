//! #8882: a module-level class constructing a sibling class that is declared
//! inside a function body lowered LATER. This is the shape the CJS wrap
//! produces for Next's `server/lib/lru-cache.js`: `LRUCache` is hoisted out of
//! the module IIFE while `SentinelNode` (whose doc comment closes on the
//! `class` line, so the textual hoister never sees it) stays inside the
//! `__perry_cjs_factory` closure. JS binds the constructor reference when the
//! `new` executes; the #8643 guard instead lowered it to an unconditional,
//! nameless `ReferenceError` that killed the application at init.

#[test]
fn hoisted_class_constructs_sibling_declared_inside_a_later_closure() {
    let source = r#"
        class LRUCache {
            constructor() {
                this.head = new SentinelNode();
                this.tail = new SentinelNode();
            }
        }
        const _cjs = (function () {
            class SentinelNode {
                constructor() {
                    this.prev = null;
                    this.next = null;
                }
            }
            return { SentinelNode };
        })();
    "#;
    let module = perry_parser::parse_typescript(source, "lru-cache.js").expect("source parses");
    let hir = super::lower_module(&module, "lru-cache", "lru-cache.js").expect("source lowers");
    let lru_cache = hir
        .classes
        .iter()
        .find(|class| class.name == "LRUCache")
        .expect("LRUCache class is lowered");
    let debug = format!("{lru_cache:?}");

    assert!(
        !debug.contains("js_throw_reference_error_unresolved_get")
            && !debug.contains("js_global_get_or_throw_unresolved"),
        "a sibling class declared later in the module must not lower to a \
         compile-time ReferenceError:\n{debug}"
    );
    assert_eq!(
        debug.matches(r#"New { class_name: "SentinelNode""#).count(),
        2,
        "both `new SentinelNode()` sites must stay late-bound by-name constructs:\n{debug}"
    );
}
