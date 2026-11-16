//! ECMA-262 7.1.19 ToPropertyKey of a Number is a numeric string: `f[i]`
//! with a number `i` reads only property `i` of `f` (own, then its chain).
//! A function value (program function, class, prototype, bound or built-in
//! function) keeps every property the program wrote in its field table, so
//! a number key reads its numeric fields, never a named one. three.js
//! GLTFLoader `children[i]`, where `children` may hold
//! `Object.prototype.hasOwnProperty` (a JSON key), read that function's
//! `spot` field into `Object3D.add`.
use super::*;

fn src(body: &str, read: &str) -> String {
    format!(
        "class Box {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         class Spot {{ k = 1; }}
         const i: number = process.argv.length > 9 ? 2 : 1;
         {body}
         const b = new Box({read});
         console.log(b.v);"
    )
}

/// Whether `b.v` may hold a `Spot`, or is ⊤.
fn spot(s: &str) -> bool {
    let pf = facts(s);
    let k = &field(tmpl(&pf, "Box"), "v").kind;
    k.top || k.objs.iter().any(|o| o.contains("Spot"))
}

const BUILTIN: &str = "const g: any = Object.prototype.hasOwnProperty; g.spot = new Spot();";
const PROGRAM: &str = "function f() {} const g: any = f; g.spot = new Spot();";

#[test]
fn number_key_skips_named_field_of_builtin_fn() {
    assert!(!spot(&src(BUILTIN, "g[i]")));
}

#[test]
fn number_key_skips_named_field_of_program_fn() {
    assert!(!spot(&src(PROGRAM, "g[i]")));
}

#[test]
fn string_key_reads_named_field() {
    assert!(spot(&src(BUILTIN, "g[process.argv[2]]")));
    assert!(spot(&src(PROGRAM, "g[process.argv[2]]")));
}

#[test]
fn number_key_reads_numeric_field() {
    assert!(spot(&src(
        "const g: any = Object.prototype.hasOwnProperty; g[0] = new Spot();",
        "g[i]"
    )));
    assert!(spot(&src("function f() {} const g: any = f; g[0] = new Spot();", "g[i]")));
}

#[test]
fn unknown_code_value_stays_open() {
    assert!(spot(&src(
        "const g: any = new Function(process.argv[2])(); g.spot = new Spot();",
        "g[i]"
    )));
}
