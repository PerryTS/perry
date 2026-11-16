//! ARRABSENT: a named key the program never stores on an array, that is no
//! array index and not `length`, is absent on it: an Array exotic object's
//! own keys are its indices and `length` (ECMA-262 10.4.2, its
//! [[DefineOwnProperty]] is special only for those), every other key is an
//! ordinary property the program would have to store, and [[Get]] is
//! OrdinaryGet (10.1.8.1): it walks `%Array.prototype%` and
//! `%Object.prototype%`. When neither describes the key and nothing writes
//! it there, the read gives `undefined`, as `absent_key` finds for a unit.
//! `%Array.prototype%` takes named adds (perry-abi lock.rs `add_locked`),
//! so the claim needs a realm no unknown code holds.
//! (i) an array beside a literal; (ii) a typed array beside a literal (a
//! builtin object: not this rule); (iii) cloneUniforms: class instances and
//! an array literal. Controls: a stored named key, `length`, an index, an
//! Array.prototype member, a program add to Array.prototype, unknown
//! code (held realm), `arguments`, a Promise.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// Verdicts of the reads of `key`, in source order.
fn reads(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && !s.is_call).collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("LITFACT {key}: {v:?}");
    v
}

const OWN: &str = "OwnDataOrAbsent";
const C: &str = "const c = Math.random() > 0.5;\n";

fn all_own(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|v| v == OWN)
}
fn none_own(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|v| v != OWN)
}

#[test]
fn i_array_beside_a_literal_is_absent() {
    let pf = one(&format!(
        "{C}const v: any = c ? {{ POSITION: 1 }} : [1, 2];\nconsole.log(v.POSITION);\n"
    ));
    let v = reads(&pf, "POSITION");
    assert!(all_own(&v), "{v:?}");
}

#[test]
fn ii_typed_array_beside_a_literal_stays_open() {
    // A typed array is a builtin object (`TmplKind::BObj`), not this rule.
    let pf = one(&format!(
        "{C}const v: any = c ? {{ POSITION: 1 }} : new Float32Array(2);\nconsole.log(v.POSITION);\n"
    ));
    let v = reads(&pf, "POSITION");
    assert!(none_own(&v), "{v:?}");
}

#[test]
fn iii_clone_uniforms_instances_and_array_literal() {
    let pf = one(&format!(
        "{C}class Color {{ isColor: boolean; r: number; constructor() {{ this.isColor = true; this.r = 0; }} }}\n\
         class Vec {{ x: number; constructor() {{ this.x = 0; }} }}\n\
         function f(p: any) {{ return p.isColor; }}\n\
         console.log(f(new Color()), f(new Vec()), f([1, 2, 3]));\n"
    ));
    let v = reads(&pf, "isColor");
    assert!(all_own(&v), "{v:?}");
}

/// The array's own keys are data (`a.POSITION = 5` adds a named data key):
/// own data or `undefined` holds (`json_chain_array`), no longer refused.
#[test]
fn control_named_store_on_the_array() {
    let pf = one(&format!(
        "{C}const a: any = [1, 2];\na.POSITION = 5;\n\
         const v: any = c ? {{ POSITION: 1 }} : a;\nconsole.log(v.POSITION);\n"
    ));
    let v = reads(&pf, "POSITION");
    assert!(all_own(&v), "{v:?}");
}

/// An Array's `length` is its own data (10.4.2): own data or `undefined`
/// holds (`json_chain_array`), no longer refused.
#[test]
fn control_length_is_not_this_rule() {
    let pf = one(&format!(
        "{C}const v: any = c ? {{ a: 1 }} : [1, 2];\nconsole.log(v.length);\n"
    ));
    let v = reads(&pf, "length");
    assert!(all_own(&v), "{v:?}");
}

#[test]
fn control_index_key_is_not_this_rule() {
    let pf = one(&format!(
        "{C}const v: any = c ? {{ a: 1 }} : [1, 2];\nconsole.log(v[\"0\"]);\n"
    ));
    let v = reads(&pf, "0");
    assert!(v.iter().all(|v| v != OWN), "{v:?}");
}

#[test]
fn control_array_prototype_member() {
    let pf = one(&format!(
        "{C}const v: any = c ? {{ a: 1 }} : [1, 2];\nconsole.log(v.map);\n"
    ));
    let v = reads(&pf, "map");
    assert!(none_own(&v), "{v:?}");
}

#[test]
fn control_program_adds_the_key_to_array_prototype() {
    let pf = one(&format!(
        "{C}(Array.prototype as any).POSITION = 7;\n\
         const v: any = c ? {{ POSITION: 1 }} : [1, 2];\nconsole.log(v.POSITION);\n"
    ));
    let v = reads(&pf, "POSITION");
    assert!(none_own(&v), "{v:?}");
}

#[test]
fn control_unknown_code_holds_the_realm() {
    // Unknown code (a declared function: ECMA-262 9.3, it holds the realm)
    // may run `Array.prototype.POSITION = 7`, which the lock admits.
    let pf = one(&format!(
        "{C}declare function sink(x: any): void;\nsink(0);\n\
         const v: any = c ? {{ POSITION: 1 }} : [1, 2];\nconsole.log(v.POSITION);\n"
    ));
    let v = reads(&pf, "POSITION");
    assert!(none_own(&v), "{v:?}");
}

#[test]
fn control_arguments_beside_a_literal() {
    let pf = one(&format!(
        "{C}function g(..._x: any[]): any {{ return arguments; }}\n\
         const v: any = c ? {{ POSITION: 1 }} : g(1);\nconsole.log(v.POSITION);\n"
    ));
    let v = reads(&pf, "POSITION");
    assert!(none_own(&v), "{v:?}");
}

#[test]
fn control_promise_beside_a_literal() {
    let pf = one(&format!(
        "{C}const v: any = c ? {{ POSITION: 1 }} : Promise.resolve(1);\nconsole.log(v.POSITION);\n"
    ));
    let v = reads(&pf, "POSITION");
    assert!(none_own(&v), "{v:?}");
}
