//! A sloppy function's `this` for a primitive this-value is ToObject of it
//! (ECMA-262 10.2.1.2 OrdinaryCallBindThis step 6, 7.1.18): a fresh
//! Number / String / Boolean wrapper object, never the unknown value. A
//! store `this.x = a` is a store on that wrapper. A strict function's
//! `this` stays the primitive (step 5). (three.js `Material.call(v)` with a
//! JSON-parsed `v`.)

use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

fn src(body: &str, calls: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         class A {{ a = 1; }}
         const a = new A();
         function M(this: any) {{ {body} }}
         {calls}
         unknown.take({{ q: 1 }});
         console.log(a.a);"
    )
}

#[test]
fn a_primitive_this_of_a_sloppy_function_is_a_wrapper() {
    let pf = whole(&src(
        "this.x = a;",
        "M.call(5); M.call('s'); M.call(true); M.call(JSON.parse(process.argv[3] ?? '0'));",
    ));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

/// Control: the store lands on the wrapper, which the function returns.
#[test]
fn the_wrapper_holds_the_store() {
    let pf = whole(&src(
        "this.x = a; return this;",
        "const w: any = M.call(5); unknown.take(w.x);",
    ));
    assert!(escape_with_field(&pf, "a").is_some(), "{}", pf.dump_text());
}

/// Control: a `this` unknown code makes stays open.
#[test]
fn an_unknown_this_stays_open() {
    let pf = whole(&src(
        "this.x = a;",
        "M.call(new Function(process.argv[3])());",
    ));
    assert!(escape_with_field(&pf, "a").is_some(), "{}", pf.dump_text());
}

/// Control: a strict function's `this` is the primitive itself.
#[test]
fn a_strict_function_keeps_the_primitive_this() {
    let pf = whole(&src(
        "'use strict'; if (typeof this === 'number') { console.log(1); } else { this.x = a; }",
        "M.call(5); M.call('s');",
    ));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}
