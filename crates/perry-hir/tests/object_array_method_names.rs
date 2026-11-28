use perry_hir::lower_module;
use perry_parser::parse_typescript;

#[test]
fn local_object_callback_methods_are_not_array_builtins() {
    for method in [
        "forEach", "map", "filter", "some", "every", "reduce", "sort",
    ] {
        for args in ["[1, 2]", "[1, 2], (value) => value"] {
            let source = format!(
                "function impl(items, callback) {{ return items; }} \
                 var facade = {{ {method}: impl }}; \
                 facade.{method}({args});"
            );
            let parsed = parse_typescript(&source, "object_methods.js").unwrap();
            let hir = format!(
                "{:?}",
                lower_module(&parsed, "test", "object_methods.js").unwrap()
            );
            for array_op in [
                "ArrayLikeMethod",
                "ArrayForEach",
                "ArrayMap",
                "ArrayFilter",
                "ArraySome",
                "ArrayEvery",
                "ArrayReduce",
                "ArraySort",
            ] {
                assert!(
                    !hir.contains(array_op),
                    "plain-object {method}({args}) became {array_op}: {hir}"
                );
            }
        }
    }
}

#[test]
fn actual_arrays_keep_their_callback_lowering() {
    let parsed = parse_typescript(
        "const items = [1, 2]; items.forEach(value => value); \
         items.map(value => value, {});",
        "array.js",
    )
    .unwrap();
    let hir = format!("{:?}", lower_module(&parsed, "test", "array.js").unwrap());
    assert!(hir.contains("ArrayForEach"));
    assert!(hir.contains("ArrayLikeMethod"));
}

/// Every `PropertySet` in the lowered module that names `property`.
fn property_sets_of(hir: &str, property: &str) -> usize {
    let needle = format!("property: \"{property}\"");
    hir.match_indices("PropertySet {")
        .filter(|(at, _)| {
            let rest = &hir[at + "PropertySet {".len()..];
            let end = rest.find("PropertySet {").unwrap_or(rest.len());
            rest[..end.min(400)].contains(&needle)
        })
        .count()
}

#[test]
fn statement_splice_on_member_never_writes_the_member_back() {
    // Array.prototype.splice mutates its this value and returns the removed
    // elements; the expression statement discards that value. No PutValue on
    // `this.items` happens, so a getter-only accessor must not be assigned.
    let parsed = parse_typescript(
        "class M { constructor() { this._items = [1, 2]; } \
           get items() { return this._items; } \
           add(x) { this.items.splice(1, 0, x); return this.items.join(','); } } \
         class F { constructor() { this.items = [1, 2]; } \
           add(x) { this.items.splice(0, 1); } }",
        "splice_member.js",
    )
    .unwrap();
    let hir = format!(
        "{:?}",
        lower_module(&parsed, "test", "splice_member.js").unwrap()
    );
    assert!(!hir.contains("__splice_temp"), "{hir}");
    // Field initialization is not a PropertySet; any one here is a write-back.
    assert_eq!(property_sets_of(&hir, "items"), 0, "{hir}");
}
