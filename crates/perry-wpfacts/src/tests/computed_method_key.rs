//! A class method under a computed key that names one property key
//! (`*[Symbol.iterator]()`, `["k"]()`) is defined under exactly that key
//! (ECMA-262 15.4.4 MethodDefinitionEvaluation, 10.2.8
//! DefineMethodProperty): a read of another key never yields it. (three.js
//! `Vector3`'s `*[Symbol.iterator]()`: `_vector.x` read the generator
//! function, and `attr.setXYZ(i, _vector.x, ...)` stored it into a typed
//! array the model does not hold.) The method's escape is observed
//! through the `W` its body yields to whoever runs it.

use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.cts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn escape_of(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.cts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.cts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

fn src(key: &str, stored: &str) -> String {
    format!(
        "const unknown: any = new Function(process.argv[2])();
         class W {{ w = 1; }}
         class V {{ x = 1; *[{key}]() {{ yield new W(); }} }}
         const v: any = new V();
         unknown[0] = {stored};
         console.log(v.x);"
    )
}

#[test]
fn a_symbol_keyed_method_is_not_read_under_another_key() {
    let pf = whole(&src("Symbol.iterator", "v.x"));
    assert_eq!(escape_of(&pf, "w"), None, "{}", pf.dump_text());
}

#[test]
fn a_string_keyed_method_is_not_read_under_another_key() {
    let pf = whole(&src("'it'", "v.x"));
    assert_eq!(escape_of(&pf, "w"), None, "{}", pf.dump_text());
}

/// Control: a key known only at run time may be "x": unknown code gets
/// the generator function and runs it, so the `W` it yields escapes.
#[test]
fn an_unknown_key_stays_open() {
    let pf = whole(&src("process.argv[3]", "v.x"));
    assert!(escape_of(&pf, "w").is_some(), "{}", pf.dump_text());
}
