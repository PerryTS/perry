//! ECMA-262 7.1.19 ToPropertyKey of a Number is a numeric string: `F[i]`
//! with a number `i` reads only property `i` of a built-in constructor `F`
//! (own, then its chain). A built-in constructor is a function object
//! (`Object`, `Array`, ...): it keeps the properties the program wrote in
//! its field table, so a number key reads its numeric fields, never a
//! named one. three.js GLTFLoader `children[i]`, where `children` may hold
//! the `Object` constructor (`nodeCache[k].constructor` under a JSON key),
//! read `Object.spot` (GLTFLights `lightDef.spot`) into `Object3D.add`.
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

fn kind(s: &str) -> (bool, Vec<String>) {
    let pf = facts(s);
    let k = &field(tmpl(&pf, "Box"), "v").kind;
    (k.top, k.objs.iter().map(|o| o.to_string()).collect())
}

/// Whether `b.v` may hold a `Spot`, or is ⊤.
fn spot(s: &str) -> bool {
    let (top, objs) = kind(s);
    top || objs.iter().any(|o| o.contains("Spot"))
}

const OBJECT: &str = "const g: any = Object; g.spot = new Spot();";
const ARRAY: &str = "const g: any = Array; g.spot = new Spot();";

#[test]
fn number_key_skips_named_field_of_object_ctor() {
    assert!(!spot(&src(OBJECT, "g[i]")));
}

#[test]
fn number_key_skips_named_field_of_array_ctor() {
    assert!(!spot(&src(ARRAY, "g[i]")));
}

#[test]
fn string_key_reads_named_field() {
    assert!(spot(&src(OBJECT, "g[process.argv[2]]")));
    assert!(spot(&src(ARRAY, "g[process.argv[2]]")));
}

#[test]
fn number_key_reads_numeric_field() {
    assert!(spot(&src("const g: any = Object; g[0] = new Spot();", "g[i]")));
    assert!(spot(&src("const g: any = Array; g[i] = new Spot();", "g[i]")));
}

#[test]
fn unknown_code_value_stays_open() {
    assert!(spot(&src(
        "const g: any = new Function(process.argv[2])(); g.spot = new Spot();",
        "g[i]"
    )));
}

/// A `JSON.parse(process.argv[2])` value used as a key may be
/// "constructor": `m.p` keeps the built-in member's receivers beside `A`.
#[test]
fn json_key_still_reads_constructor() {
    let pf = facts(
        "class A { p = 1; }
         const o: any = {};
         const k: any = JSON.parse(process.argv[2]);
         let m: any = o[k];
         if (m === undefined) { m = new A(); }
         console.log(m.p);",
    );
    let r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter().cloned())
        .collect();
    assert!(r.iter().any(|x| !x.contains("A")), "{r:?}");
}
