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
