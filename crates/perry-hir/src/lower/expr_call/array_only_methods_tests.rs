//! Verdict tests for array-method lowering on nested property receivers.

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &src,
                "nested_array_method.ts",
                &mut cache,
            )
            .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "nested_array_method.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn nested_object_map_with_two_args_uses_own_method() {
    let hir = format!(
        "{:?}",
        lower(
            r#"
            function render(children: any[], callback: (value: any) => any) {
                const react = {
                    default: {
                        Children: {
                            map(items: any[], fn: (value: any) => any) {
                                return items.map(fn);
                            },
                        },
                    },
                };
                return react.default.Children.map(children, callback);
            }
            "#,
        )
    );

    assert!(
        !hir.contains("ArrayLikeMethod"),
        "an object's own map method was lowered as Array.prototype.map: {hir}"
    );
    assert_eq!(
        hir.matches("ArrayMap").count(),
        1,
        "only map() inside the facade implementation is an Array map: {hir}"
    );
}

#[test]
fn typed_nested_array_field_keeps_array_map_specialization() {
    let hir = format!(
        "{:?}",
        lower(
            r#"
            class Box {
                items: number[] = [1, 2];
                run() {
                    return this.items.map((value) => value * 2);
                }
            }
            "#,
        )
    );

    assert!(
        hir.contains("ArrayMap"),
        "a statically typed Array field should retain the dense fast path: {hir}"
    );
}
