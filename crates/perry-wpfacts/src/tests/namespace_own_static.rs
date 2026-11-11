//! A namespace object's own static ends the lookup (ECMA-262 10.1.8.1
//! OrdinaryGet: when O.[[GetOwnProperty]](P) is a property, its value is
//! the result; the prototype is read only when O has no own P).
//! `Promise.all` is an own data property of %Promise% (27.2.4.1): a key a
//! store wrote onto `Object.prototype` (or `Function.prototype`) under any
//! name is never what `Promise.all` reads. skinbench: GLTFLoader's
//! `Promise.all( defs.map( ... ) )` (getDependencies) read `all` through
//! the `Object.prototype` gate that three's `Vector3.toArray( array,
//! offset )` (`array[ offset ] = this.x`, receiver and key ⊤) opens: the
//! callee was ⊤ and the iterable escaped.
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

fn src(tail: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         const kept = {{ keptmark: 1 }};
         const arr: any = [1, 2];
         arr.k = kept;
         {tail}
         console.log(String(arr.k === kept));"
    )
}

/// `Promise.all` after a store into ⊤ under an unknown key (skinbench:
/// `Vector3.toArray`'s `array[ offset ] = this.x`, receiver and key
/// unknown, so it may write `all` onto `Object.prototype`) while the
/// program also holds an opaque builtin function value, which may be
/// `Promise`: the read is on `Promise`, which owns `all`; the own static
/// iterates its argument.
#[test]
fn promise_all_beside_an_opaque_builtin_function_is_the_own_static() {
    let pf = whole(&src(
        "const g: any = (Array.prototype as any)[process.argv[3]]; console.log(typeof g); \
         function put(o: any, k: any) { o[k] = 1; } unknown(put); \
         Promise.all(arr).then(() => console.log(\"ok\"));",
    ));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// The same, with the store into ⊤ made by an escaped function (receiver
/// and key unknown, as `Vector3.toArray`'s `array[ offset ] = this.x`).
#[test]
fn promise_all_after_a_store_into_top_is_the_own_static() {
    let pf = whole(&src(
        "function put(o: any, k: any) { o[k] = 1; } unknown(put); \
         Promise.all(arr).then(() => console.log(\"ok\"));",
    ));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// Control: a store under an unknown key onto `Promise` itself may
/// replace `all`: the callee is unknown, the iterable escapes.
#[test]
fn promise_all_after_a_store_onto_promise_escapes_the_argument() {
    let pf = whole(&src(
        "(Promise as any)[process.argv[4]] = unknown; Promise.all(arr);",
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Control: the iterable handed to `new Function` code escapes.
#[test]
fn an_iterable_given_to_unknown_code_escapes() {
    let pf = whole(&src("new Function(process.argv[4])()(Promise.all, arr);"));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
