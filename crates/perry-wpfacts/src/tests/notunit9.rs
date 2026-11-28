//! Rule R-IN (ECMA-262 13.10.1, 7.3.12 HasProperty, 10.1.7.1
//! OrdinaryHasProperty): `k in o` is `true` only when some value of `o` may
//! have `k` (an own value under `k` or under an unknown key, a prototype
//! member, an intrinsic's declared key, a Proxy's `has` trap) and `false`
//! only when some value may lack it; the dead-arm rule (DEADARM) then closes
//! an arm whose `in` test is never `true`. RED on 56968a1983: `in` always
//! gave both booleans, so three.js uploadTexture's
//! `'texElementImage2D' in _gl` arm (no context has that method) appended
//! every texture image to the canvas (NOTUNIT8 chain step 2).
use super::*;

fn kind(pf: &ProgramFacts, key: &str) -> crate::derive::KindFact {
    let fs: Vec<_> = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .filter(|f| f.key == key)
        .collect();
    assert!(!fs.is_empty(), "no field {key}");
    fs[0].kind.clone()
}

/// `test` guards the store of an object into `holder.direct`.
fn src(pre: &str, test: &str) -> String {
    format!(
        r#"
class Gl {{ clear() {{ return 1; }} }}
const g: any = {{ a: 1 }};
const gl = new Gl();
const holder: any = {{ direct: null }};
{pre}
function up(img: any) {{
  if ({test}) {{
    holder.direct = img;
  }}
}}
up({{ w: 1 }});
console.log(holder.direct);
"#
    )
}

fn direct_live(pre: &str, test: &str) -> bool {
    let pf = facts(&src(pre, test));
    let k = kind(&pf, "direct");
    k.top || !k.objs.is_empty()
}

/// RED: `g` has no `zz` anywhere on its chain: the arm never runs.
#[test]
fn rin_absent_key_closes_the_arm() {
    assert!(!direct_live("", "'zz' in g"));
}

/// RED: a class instance without the method (three.js
/// `'texElementImage2D' in _gl`).
#[test]
fn rin_absent_method_on_class_instance_closes_the_arm() {
    assert!(!direct_live("", "'texElementImage2D' in gl"));
}

/// Control: an own key.
#[test]
fn rin_own_key_keeps_the_arm() {
    assert!(direct_live("", "'a' in g"));
}

/// Control: a key stored later (the answer re-asked as fields grow).
#[test]
fn rin_key_stored_later_keeps_the_arm() {
    assert!(direct_live("setTimeout(() => { g.zz = 2; }, 0);", "'zz' in g"));
}

/// Control: a store of `undefined` still makes the key.
#[test]
fn rin_undefined_store_keeps_the_arm() {
    assert!(direct_live("g.zz = undefined;", "'zz' in g"));
}

/// Control: a store under an unknown key may be `zz` (a String of a
/// Number is no such key: it stores under a numeric name).
#[test]
fn rin_any_key_store_keeps_the_arm() {
    assert!(direct_live(
        "const k = process.argv[2] as string; g[k] = 2;",
        "'zz' in g"
    ));
}

/// Control: a prototype method of the class.
#[test]
fn rin_proto_member_keeps_the_arm() {
    assert!(direct_live("", "'clear' in gl"));
}

/// Control: a key `Object.prototype` declares.
#[test]
fn rin_object_prototype_key_keeps_the_arm() {
    assert!(direct_live("", "'toString' in g"));
}

/// Control: a builtin object's declared key.
#[test]
fn rin_builtin_declared_key_keeps_the_arm() {
    assert!(direct_live("", "'log' in console"));
}

/// Control: a Proxy runs its `has` trap.
#[test]
fn rin_proxy_has_trap_keeps_the_arm() {
    assert!(direct_live(
        "const p = new Proxy({}, { has() { return true; } });",
        "'zz' in p"
    ));
}

/// Control: a computed key of no known name.
#[test]
fn rin_unknown_key_keeps_the_arm() {
    assert!(direct_live("const q = process.argv[2] as string;", "q in g"));
}

/// Control: the negation keeps its arm (`false` for a lacking key).
#[test]
fn rin_negated_absent_key_keeps_the_arm() {
    assert!(direct_live("", "!('zz' in g)"));
}

/// Control: a store under some Number makes every numeric name (`NUM_KEY`).
#[test]
fn rin_number_key_store_keeps_a_numeric_name() {
    assert!(direct_live("g[Number(process.argv[2])] = 2;", "'3' in g"));
}

/// The String of a Number names a numeric key only: a non-numeric name
/// stays absent.
#[test]
fn rin_number_key_store_keeps_other_names_absent() {
    assert!(!direct_live("g[Number(process.argv[2])] = 2;", "'zz' in g"));
}
