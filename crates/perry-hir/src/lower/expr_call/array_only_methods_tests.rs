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

/// #11187: a call result whose static type is `any` is not an Array, so
/// `.sort(obj)` / `.flat()` / `.toSpliced()` / `.map(f)` on it must reach
/// the receiver's own method through dynamic dispatch (mongodb's
/// `collection.find({}).sort({ a: 1 })`).
#[test]
fn any_call_result_receiver_does_not_fold_to_array_methods() {
    let hir = format!(
        "{:?}",
        lower(
            r#"
            function run(holder: any, flag: boolean, other: any) {
                holder.find().sort({ a: 1 });
                holder.find().flat();
                holder.find().toSpliced(0, 1);
                (flag ? holder : other).map((x: any) => x);
            }
            "#,
        )
    );

    for folded in ["ArraySort", "ArrayFlat", "ArrayToSpliced", "ArrayMap"] {
        assert!(
            !hir.contains(folded),
            "an unproven receiver was folded to {folded}: {hir}"
        );
    }
}

/// #11187 control: receivers proven to be Arrays keep the dense folds.
#[test]
fn proven_array_call_results_keep_array_folds() {
    let hir = format!(
        "{:?}",
        lower(
            r#"
            function nums(): number[] { return [3, 1, 2]; }
            function run(s: string) {
                nums().sort((a, b) => a - b);
                nums().map((n) => [n]).flat();
                nums().toSpliced(0, 1);
                s.split(",").sort((a, b) => (a < b ? -1 : 1));
                [1, 2].map((n) => n).sort((a, b) => a - b);
            }
            "#,
        )
    );

    assert_eq!(hir.matches("ArraySort").count(), 3, "{hir}");
    assert!(hir.contains("ArrayFlat"), "{hir}");
    assert!(hir.contains("ArrayToSpliced"), "{hir}");
}
