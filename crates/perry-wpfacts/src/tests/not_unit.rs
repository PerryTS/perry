//! NOTUNIT (U5): a named read whose receiver set holds `JSON.parse` values.
//! A parse without a reviver makes ordinary objects whose [[Prototype]] is
//! `%Object.prototype%` and whose own keys are data properties
//! (ECMA-262 25.5.1 JSON.parse, ParseJSON: the text is evaluated as an
//! ObjectLiteral / ArrayLiteral; 13.2.5.5 PropertyDefinitionEvaluation
//! uses CreateDataPropertyOrThrow, and JSON text is not subject to
//! Annex B.3.1, so even `__proto__` is an own data key), arrays
//! (ArrayCreate, 10.4.2) and primitives that are strings, numbers,
//! booleans or null, nothing else. A read of a key `%Object.prototype%`
//! lacks (and nothing writes there) is the object's own data or
//! `undefined`: OwnDataOrAbsent, with the String/Number/Boolean families
//! by `family_absent` and `undefined`/`null` naming the TypeError arm.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn at(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call == call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?} {:?}", s.verdict, s.receivers)).collect();
    eprintln!("NOTUNIT {key} call={call}: {v:?}");
    v.into_iter().map(|v| v.split(' ').next().unwrap_or("").to_string()).collect()
}

fn all(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v == k)
}
fn none(v: &[String], k: &str) -> bool {
    !v.is_empty() && v.iter().all(|v| v != k)
}

const C: &str = "const c = Math.random() > 0.5;\nconst s = String(Math.random());\n";
const HOLD: &str = "declare function hostTake(x: any): void;\nhostTake(0);\n";

#[test]
fn u5_root_of_a_parse_absent_key() {
    let pf = one(&format!("{C}const o: any = JSON.parse(s);\nconsole.log(o.ior);\n"));
    assert!(all(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// A field of a parse: objects, arrays, strings, numbers, booleans, null,
/// and `undefined` for an absent key; the nullish values name their arm.
#[test]
fn u5_field_of_a_parse_names_the_type_error_arm() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s);\nconst e: any = o.extensions;\nconsole.log(e.ior);\n"
    ));
    assert!(all(&at(&pf, "ior", false), "OwnDataOrAbsent"));
    assert!(pf
        .sites
        .iter()
        .filter(|s| s.key == "ior")
        .all(|s| s.receivers.iter().any(|r| r.ends_with("(TypeError)"))));
}

/// Keys `%Object.prototype%` has: the read may give its member.
#[test]
fn control_object_prototype_keys() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s);\nconsole.log(o.constructor, o.toString, o.hasOwnProperty);\n"
    ));
    for k in ["constructor", "toString", "hasOwnProperty"] {
        assert!(none(&at(&pf, k, false), "OwnDataOrAbsent"), "{k}");
    }
}

#[test]
fn control_object_prototype_written() {
    let pf = one(&format!(
        "{C}(Object.prototype as any).ior = 1;\nconst o: any = JSON.parse(s);\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// A program object with an accessor under the key beside the parse.
#[test]
fn control_program_object_mixed_in() {
    let pf = one(&format!(
        "{C}const o: any = c ? JSON.parse(s) : {{ get ior() {{ return 1; }} }};\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// Unknown code holds the realm and the value: it may define an accessor
/// on the object or set its prototype, and String.prototype takes adds.
#[test]
fn control_held_realm_and_escape() {
    let pf = one(&format!(
        "{C}{HOLD}const o: any = JSON.parse(s);\nhostTake(o);\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
    let pf = one(&format!(
        "{C}{HOLD}const o: any = JSON.parse(s);\nconst e: any = o.extensions;\nconsole.log(e.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// A reviver's results are stored in the tree (25.5.1.1
/// InternalizeJSONProperty): one returning a Symbol puts a value there no
/// family claim covers. (A reviver returning its argument keeps the claim.)
#[test]
fn control_reviver_values() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s, (k: any, v: any) => Symbol());\nconst e: any = o.extensions;\nconsole.log(e.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// An accessor defined on the parsed object.
#[test]
fn control_define_property_accessor() {
    let pf = one(&format!(
        "{C}const o: any = JSON.parse(s);\nObject.defineProperty(o, \"ior\", {{ get() {{ return 1; }} }});\nconsole.log(o.ior);\n"
    ));
    assert!(none(&at(&pf, "ior", false), "OwnDataOrAbsent"));
}

/// NOTUNIT2 step 1: a receiver set of program objects with primitives
/// beside them. The read dispatches on each value's kind (7.3.2 GetV,
/// 6.2.5.5, 7.1.18): `undefined`/`null` throw, a String/Number/Boolean
/// reads its family. A claim covers every kind in the set: each nullish
/// kind names its TypeError arm, each other primitive its family.
fn mixed_claims_cover_the_primitives(pf: &ProgramFacts, key: &str) {
    use crate::model::prim::{BOOL, NULL, NUM, STR, UNDEF};
    let mut n = 0;
    for s in pf.sites.iter().filter(|s| s.key == key && !s.is_call) {
        n += 1;
        eprintln!(
            "NOTUNIT2 {key}: {:?} {:?} prims={:#x}",
            s.verdict, s.receivers, s.receiver_prims
        );
        if matches!(s.verdict, Verdict::No(_)) {
            continue;
        }
        for (bit, name) in [
            (UNDEF, "primitive:undefined (TypeError)"),
            (NULL, "primitive:null (TypeError)"),
            (STR, "primitive:string"),
            (NUM, "primitive:number"),
            (BOOL, "primitive:boolean"),
        ] {
            if s.receiver_prims & bit != 0 {
                assert!(
                    s.receivers.iter().any(|r| r == name),
                    "{key}: {:?} {:?} lacks {name}",
                    s.verdict,
                    s.receivers
                );
            }
        }
    }
    assert!(n > 0, "{key}: no site");
}

const KLASS: &str = "class A { x = 1; }\n";

#[test]
fn mixed_class_instance_or_undefined() {
    let pf = one(&format!(
        "{C}{KLASS}const a: any = c ? new A() : undefined;\nconsole.log(a.x);\n"
    ));
    mixed_claims_cover_the_primitives(&pf, "x");
}

#[test]
fn mixed_class_instance_or_null_absent_key() {
    let pf = one(&format!(
        "{C}{KLASS}const a: any = c ? new A() : null;\nconsole.log(a.ior);\n"
    ));
    mixed_claims_cover_the_primitives(&pf, "ior");
}

#[test]
fn mixed_class_instance_or_string() {
    let pf = one(&format!(
        "{C}{KLASS}const a: any = c ? new A() : \"s\";\nconsole.log(a.x);\n"
    ));
    mixed_claims_cover_the_primitives(&pf, "x");
}

#[test]
fn mixed_literal_or_primitives_absent_key() {
    let pf = one(&format!(
        "{C}const a: any = c ? {{ y: 1 }} : Math.random() > 0.5 ? undefined : 7;\nconsole.log(a.ior);\n"
    ));
    mixed_claims_cover_the_primitives(&pf, "ior");
}

#[test]
fn mixed_literal_or_string_own_key() {
    let pf = one(&format!(
        "{C}const a: any = c ? {{ y: 1 }} : s;\nconsole.log(a.y);\n"
    ));
    mixed_claims_cover_the_primitives(&pf, "y");
}

#[test]
fn mixed_array_or_undefined_absent_key() {
    let pf = one(&format!(
        "{C}const a: any = c ? [1, 2] : undefined;\nconsole.log(a.ior);\n"
    ));
    mixed_claims_cover_the_primitives(&pf, "ior");
}

// NOTUNIT3, JSON-RELINK-CLOSED: `t[k] = j.b[k]` over a for-in key of a
// JSON object may run the `__proto__` setter (B.2.2.1.2) with a value of
// the same parse: an object or an array relinks `t`, null ends its chain,
// any other primitive does nothing. ParseJSON made `__proto__` an own data
// key (25.5.1, CreateDataPropertyOrThrow), so the source of the store is
// that own value.
const RL: &str = "const j: any = JSON.parse(s);\nconst t: any = j.a;\nfor (const k in j.b) t[k] = j.b[k];\n";

fn null_arm(pf: &ProgramFacts, key: &str) -> bool {
    pf.sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .all(|s| s.receivers.iter().any(|r| r.contains("chain ends at null")))
}

fn granted(pf: &ProgramFacts, key: &str) -> bool {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key && !s.is_call).collect();
    eprintln!("NOTUNIT4 {key}: {:?}", v.iter().map(|s| (&s.verdict, &s.receivers)).collect::<Vec<_>>());
    !v.is_empty() && v.iter().all(|s| matches!(s.verdict, Verdict::OwnDataOrAbsent))
}

fn refused(pf: &ProgramFacts, key: &str) -> bool {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key && !s.is_call).collect();
    eprintln!("NOTUNIT4 {key}: {:?}", v.iter().map(|s| (&s.verdict, &s.receivers)).collect::<Vec<_>>());
    !v.is_empty() && v.iter().all(|s| matches!(s.verdict, Verdict::No(_)))
}

/// The rule reads the chain: JSON objects' data, arrays, the end
/// (`Object.prototype` lacking the key, or null, the null-ended arm).
/// ParseJSON may give an array at `j.a` (the parse's array token sits
/// beside the object token), so `t[k] = j.b[k]` may also store onto that
/// array (a named own data key; under an index an element; under `length`
/// ArraySetLength, 10.4.2.4; under `__proto__` the runtime adds an own
/// named data key), and the array may be the new prototype. Its own keys
/// are data and `%Array.prototype%` lacks `x`: both reads are own data or
/// `undefined`, the null end named.
#[test]
fn relink_to_json_values_target_may_be_an_array_is_closed() {
    let pf = one(&format!("{C}{RL}console.log(t.x);\nconsole.log(j.c.x);\n"));
    assert!(granted(&pf, "x"));
    assert!(null_arm(&pf, "x"));
}

/// Control (i): `map` is an `%Array.prototype%` member: refused.
#[test]
fn control_relink_array_prototype_member_refused() {
    let pf = one(&format!("{C}{RL}console.log(t.map);\n"));
    assert!(refused(&pf, "map"));
}

/// Control (iii): the program writes `x` onto `%Array.prototype%`: refused.
#[test]
fn control_relink_array_prototype_written_refused() {
    let pf = one(&format!("{C}(Array.prototype as any).x = 1;\n{RL}console.log(t.x);\n"));
    assert!(refused(&pf, "x"));
}

/// Control: a program object may become the prototype: refused.
#[test]
fn control_relink_to_a_program_object() {
    let pf = one(&format!(
        "{C}class A {{ x = 1; }}\nconst j: any = JSON.parse(s);\nconst t: any = j.a;\nfor (const k in j.b) t[k] = c ? j.b[k] : new A();\nconsole.log(t.x);\n"
    ));
    assert!(none(&at(&pf, "x", false), "OwnDataOrAbsent"));
}

/// Control (iv): under `length` the objects and arrays are closed (an
/// array's `length` is its own data, a number; OwnDataOrAbsent states own
/// data or `undefined`, no value set), but `t` may be a JSON string, whose
/// `length` is the String wrapper's own key (10.4.3), which the primitive
/// rule does not describe: refused, by the primitive.
#[test]
fn control_relink_to_an_array_reads_its_length() {
    let pf = one(&format!("{C}{RL}console.log(t.length);\n"));
    assert!(refused(&pf, "length"));
    assert!(pf.sites.iter().filter(|s| s.key == "length" && !s.is_call).all(|s| matches!(
        &s.verdict,
        Verdict::No(r) if r.contains("primitive beside an object")
    )));
}

/// A call on the relinked object: the JSON call claim refuses (a call).
#[test]
fn relink_call_stays_the_json_call_refusal() {
    let pf = one(&format!("{C}{RL}try {{ t.m(); }} catch (e) {{}}\n"));
    assert!(pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call)
        .all(|s| matches!(&s.verdict, Verdict::No(r) if r.contains("json: a call"))));
}

/// NOTUNIT5, GLTFLoader.js:731 + :2345: the program stores an object
/// literal under a parse object's key (`lightDef.spot = lightDef.spot ||
/// {}`), and the relinking store's value `j.b[k]` reads the parse's
/// objects under any key, so the literal may become `t`'s prototype. Its
/// own keys are data and its chain (`%Object.prototype%`) lacks `x`.
const LS: &str = "const d: any = j.d;\nd.spot = d.spot || {};\nd.spot.inner = 0;\n";

#[test]
fn relink_chain_through_a_program_literal_is_closed() {
    let pf = one(&format!("{C}const j: any = JSON.parse(s);\n{LS}const t: any = j.a;\nfor (const k in j.b) t[k] = j.b[k];\nconsole.log(t.x);\n"));
    assert!(all(&at(&pf, "x", false), "OwnDataOrAbsent"));
}

/// Control: the literal holds `x` (`d.spot.x = 1`): the read ends on the
/// literal with its own data value, or on a JSON object's data, or gives
/// `undefined`: still OwnDataOrAbsent (the verdict names data, never a
/// getter), exactly as for a JSON object on the chain that holds the key.
#[test]
fn control_relink_chain_literal_holds_the_key() {
    let pf = one(&format!("{C}const j: any = JSON.parse(s);\n{LS}d.spot.x = 1;\nconst t: any = j.a;\nfor (const k in j.b) t[k] = j.b[k];\nconsole.log(t.x);\n"));
    assert!(all(&at(&pf, "x", false), "OwnDataOrAbsent"));
}

/// Control: the literal holds an accessor under `x`: refused.
#[test]
fn control_relink_chain_literal_accessor_refused() {
    let pf = one(&format!("{C}const j: any = JSON.parse(s);\nconst d: any = j.d;\nd.spot = {{ get x() {{ return 1; }} }};\nconst t: any = j.a;\nfor (const k in j.b) t[k] = j.b[k];\nconsole.log(t.x);\n"));
    assert!(none(&at(&pf, "x", false), "OwnDataOrAbsent"));
}

/// Control: the literal escapes to unknown code: refused.
#[test]
fn control_relink_chain_literal_escapes() {
    let pf = one(&format!("{C}{HOLD}const j: any = JSON.parse(s);\n{LS}hostTake(d.spot);\nconst t: any = j.a;\nfor (const k in j.b) t[k] = j.b[k];\nconsole.log(t.x);\n"));
    assert!(none(&at(&pf, "x", false), "OwnDataOrAbsent"));
}
