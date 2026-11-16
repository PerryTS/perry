//! PEELK22 (Array.prototype methods are generic): a call of an
//! `Array.prototype` method whose `this` is a function, a builtin function or
//! another non-array object (ECMA-262 23.1.3, "intentionally generic":
//! ToObject(this), LengthOfArrayLike, Get of the present indices; the result
//! of a non-Array is a fresh Array) runs the method as on an array-like: no
//! argument reaches unknown code. A `defs.map(cb, thisArg)` whose receiver may
//! be an array or an `Object.prototype` method (GLTFLoader getDependencies
//! reads `json[<unknown string>]`) passes `thisArg` to `cb` only.
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

fn src(recv: &str) -> String {
    format!(
        "const lit: any = {{ keptmark: 1 }};
         const defs: any = process.argv.length > 99 ? [1, 2] : {recv};
         defs.map(function (this: any, v: any) {{ return this; }}, lit);"
    )
}

#[test]
fn object_proto_method_receiver_runs_map_generically() {
    let pf = whole(&src("({} as any)[process.argv[2]]"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn builtin_function_receiver_runs_map_generically() {
    let pf = whole(&src("Object.prototype.hasOwnProperty"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn program_function_receiver_runs_map_generically() {
    let pf = whole(&src("function () { return 1; }"));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn unknown_code_receiver_stays_unknown() {
    let pf = whole(&src("new Function(process.argv[2])()"));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
