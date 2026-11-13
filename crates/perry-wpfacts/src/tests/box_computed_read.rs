//! A computed read of an ordinary object a builtin makes (`Family::Box`
//! whose `members::box_chain` is `Object`: a `JSON.parse` object, a
//! property descriptor, an `Object.fromEntries` result) finds its own
//! values or `Object.prototype`'s members: `constructor` (`Object`), the
//! methods (`BuiltinNs::ObjectProtoMethod`), `__proto__` (the prototype)
//! and what a store into ⊤ wrote there; never "any builtin function".
//! (three.js GLTF JSON objects read under unknown keys: skin seed 2.)
//! A `JSON.parse` value may also be an array, whose computed read still
//! finds `Array.prototype`'s methods ("array computed key").
//! Controls: `Object.prototype` written by unknown code, the object handed
//! to unknown code, and a prototype the program gave the object.

use super::*;
use crate::model::Model;
use std::cell::RefCell;

thread_local! {
    static MINTS: RefCell<Vec<(&'static str, u32)>> = const { RefCell::new(Vec::new()) };
}

/// The generic builtin-fn mints of the last solve, by reason (explain on).
pub(crate) fn record(m: &Model) {
    let v = m.bf_mints.iter().map(|(k, n)| (*k, *n)).collect();
    MINTS.with_borrow_mut(|s| *s = v);
}

fn mints(src: &str, why: &str) -> u32 {
    let _ = facts_with(
        src,
        FactsConfig {
            explain: true,
            ..Default::default()
        },
    );
    MINTS.with_borrow(|s| s.iter().filter(|(k, _)| *k == why).map(|(_, n)| n).sum())
}

fn witness_proven(src: &str) -> (bool, String) {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    let d = pf.dump_text();
    assert!(!sites.is_empty(), "{src}\n{d}");
    let ok = sites
        .iter()
        .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set));
    (ok, format!("{src}\n{d}"))
}

const SETUP: &str = r#"
class Box { witness = 1; }
const box = new Box();
const k = "" + Math.random();
declare function ext(a: any): any;
"#;

/// The same, under a key with no shape (any string).
const SETUP_ARGV: &str = r#"
class Box { witness = 1; }
const box = new Box();
const k: string = process.argv[2];
declare function ext(a: any): any;
"#;

fn run(setup: &str, body: &str) -> (bool, String) {
    witness_proven(&format!("{setup}{body}"))
}

const F2E: &str = "const j: any = JSON.parse(\"\" + k);\nconst jf = j[k];\nconst h: any = { box };\nh.m = jf;\nh.m(\"_\");\nconsole.log(box.witness);";

/// GENFN F2e (OPAQUEFN §1 `gltfExtensions`): the computed read of the
/// JSON object mints no builtin function; the one left is the JSON
/// array's (`Array.prototype`'s methods).
#[test]
fn a_json_object_computed_read_mints_no_builtin_function() {
    for setup in [SETUP, SETUP_ARGV] {
        let src = format!("{setup}{F2E}");
        assert_eq!(mints(&src, "builtin object computed key"), 0, "{src}");
        assert!(mints(&src, "array computed key") > 0, "{src}");
    }
    let src = format!(
        "{SETUP_ARGV}const d: any = Object.getOwnPropertyDescriptor({{ q: 1 }}, \"q\");\nconst e: any = Object.fromEntries([[k, 1]]);\nconsole.log(d[k], e[k]);"
    );
    assert_eq!(mints(&src, "builtin object computed key"), 0, "{src}");
}

/// F2e on an ordinary object only (`Object.fromEntries`): the read stored
/// and called as a method of the witness's holder keeps it; so does a
/// `replace` on a value read under an unknown key.
#[test]
fn an_ordinary_builtin_object_computed_read_keeps_the_receiver() {
    for body in [
        "const j: any = Object.fromEntries([[k, 1]]);\nconst jf = j[k];\nconst h: any = { box };\nh.m = jf;\nh.m(\"_\");\nconsole.log(box.witness);",
        "const j: any = Object.fromEntries([[k, \"a b\"]]);\nj.box = box;\nconst name = j[k];\nconst r = name.replace(/\\s/g, \"_\");\nconsole.log(r, j.box.witness);",
    ] {
        let (ok, d) = run(SETUP_ARGV, body);
        assert!(ok, "{d}");
        assert!(!d.contains("unknown function value"), "{d}");
    }
}

/// GENFN F2a (guard): a named method read on a JSON object.
#[test]
fn a_json_named_read_stays_proven() {
    let (ok, d) = run(
        SETUP,
        "const j: any = JSON.parse(\"\" + k);\nj.box = box;\nconst r = j.replace(/\\s/g, \"_\");\nconsole.log(r, box.witness);",
    );
    assert!(ok, "{d}");
}

/// `__proto__` under an unknown key is `Object.prototype`: a store through
/// it is a store onto every ordinary object's chain.
#[test]
fn a_json_computed_read_may_be_object_prototype() {
    let (ok, d) = run(
        SETUP_ARGV,
        "const j: any = Object.fromEntries([[k, 1]]);\nconst p: any = j[k];\np.zz = box;\nconst o: any = {};\next(o.zz);\nconsole.log(box.witness);",
    );
    assert!(!ok, "{d}");
}

/// Controls: what may still be any function.
#[test]
fn a_computed_read_keeps_what_unknown_code_or_a_given_prototype_put_there() {
    for make in ["JSON.parse(\"\" + k)", "Object.fromEntries([[k, 1]])"] {
        for body in [
            // Object.prototype written by unknown code.
            "(Object.prototype as any)[process.argv[3]] = new Function(process.argv[4])();\nconst j: any = MAKE;\nconst h: any = { box };\nh.m = j[k];\nh.m(\"_\");\nconsole.log(box.witness);",
            // The object handed to unknown code.
            "const j: any = MAKE;\nconst u: any = new Function(\"j\", process.argv[3]);\nu(j);\nconst h: any = { box };\nh.m = j[k];\nh.m(\"_\");\nconsole.log(box.witness);",
            // A prototype the program gave the object.
            "const j: any = MAKE;\nObject.setPrototypeOf(j, { m(x: any) { ext(x); } });\nconst h: any = {};\nh.m = j[k];\nh.m(box);\nconsole.log(box.witness);",
        ] {
            let (ok, d) = run(SETUP_ARGV, &body.replace("MAKE", make));
            assert!(!ok, "{d}");
        }
    }
}

/// A computed read of a `JSON.parse` object may be `Object.prototype` (the
/// key may be `__proto__`). Defining a NAMED key on it defines that one key
/// there (node: `Object.defineProperties(o, { k: d })`), never an accessor
/// under any key (DECISIONS 242(1) is a computed key): a literal that misses
/// a key under a computed key (`lit[k]`) keeps its own reads (tsc `debug.ts`
/// `__tsDebuggerDisplay` on a value that may be Object.prototype vs
/// `Diagnostics.*` reads).
#[test]
fn a_named_define_on_a_value_that_may_be_object_prototype_defines_that_key() {
    let mut bad = Vec::new();
    for make in ["JSON.parse(\"\" + k)", "Object.fromEntries([[k, 1]])"] {
        for define in [
            "Object.defineProperties(o, { cfg: { value() { return 1; } } })",
            "Object.defineProperties(o, { cfg: { enumerable: false, value: 1 } })",
            "Object.defineProperty(o, \"cfg\", { value: 1 })",
        ] {
            let body = format!(
                "const j: any = {make};\nconst o: any = j[k];\n{define};\nconst lit: any = {{ witness: 1 }};\nconsole.log(lit[k]);\nconsole.log(lit.witness);"
            );
            let (ok, d) = run(SETUP_ARGV, &body);
            if !ok {
                bad.push(format!("{make} {define}"));
                eprintln!("{d}");
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
