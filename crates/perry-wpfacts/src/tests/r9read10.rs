// R9READ10 (2026-11-16): calls whose builtin-function receiver arm reads an
// absent key (the TypeError arm, ECMA-262 13.3.6.2 EvaluateCall, 7.3.14
// Call: IsCallable(undefined) is false), and the own `length` of a builtin
// function value (20.2.4.1).
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn facts(src: &str) -> crate::ProgramFacts {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// The verdicts of the non-store sites of `key` in a.ts.
fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    for verdict in &v {
        eprintln!("RR10 .{key} verdict={verdict}");
    }
    v
}

fn proven(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|x| !x.starts_with("No("))
}

const CALL: &str = "class V { toArray(b: any, o: number) { b[o] = 1; return b; } }
const n = Number(process.argv[2] ?? \"3\");
const x: any = n > 2 ? new V() : n > 1 ? Number : Math.max;
";
const USE: &str =
    "try { console.log(x.toArray([], 0)); } catch (e) { console.log(\"TypeError\"); }\n";

/// c1 (RED before): the call's builtin-function arms (a builtin constructor,
/// a builtin method value) read `undefined` and throw TypeError; the class
/// arm calls its own prototype method: a per-kind call verdict.
#[test]
fn r9read10_c1_call_absent_on_builtin_fn_throws() {
    let pf = facts(&format!("{CALL}{USE}"));
    assert!(proven(&sites(&pf, "toArray")));
}

/// KEEP c2: the key is written on Function.prototype.
#[test]
fn r9read10_c2_function_prototype_write_kept() {
    let pf = facts(&format!(
        "{CALL}(Function.prototype as any).toArray = function () {{ return 1; }};\n{USE}"
    ));
    assert!(!proven(&sites(&pf, "toArray")));
}

/// KEEP c3: the key is written on Object.prototype.
#[test]
fn r9read10_c3_object_prototype_write_kept() {
    let pf = facts(&format!(
        "{CALL}(Object.prototype as any).toArray = function () {{ return 1; }};\n{USE}"
    ));
    assert!(!proven(&sites(&pf, "toArray")));
}

/// KEEP c4: a getter for the key on Object.prototype.
#[test]
fn r9read10_c4_object_prototype_getter_kept() {
    let pf = facts(&format!(
        "{CALL}Object.defineProperty(Object.prototype, \"toArray\", {{ get() {{ return () => 1; }} }});\n{USE}"
    ));
    assert!(!proven(&sites(&pf, "toArray")));
}

/// KEEP c5: the builtin method value escapes to unknown code (an
/// unresolved import), which may store the key on it.
#[test]
fn r9read10_c5_escaped_builtin_fn_kept() {
    let pf = facts(&format!(
        "import {{ sink }} from \"unknown-pkg\";\n{CALL}sink(Math.max);\n{USE}"
    ));
    assert!(!proven(&sites(&pf, "toArray")));
}

/// KEEP c6: the program stores the key on the builtin method value itself
/// (c2-c5 refuse earlier: the prototype write / escape gives ⊤ receivers).
#[test]
fn r9read10_c6_store_on_builtin_fn_kept() {
    let pf = facts(&format!(
        "{CALL}(Math.max as any).toArray = function () {{ return 1; }};\n{USE}"
    ));
    let v = sites(&pf, "toArray");
    assert!(
        v.iter()
            .any(|x| x.starts_with("No(\"receiver is a builtin")),
        "{v:?}"
    );
}

const LEN: &str = "class W { length: number; constructor() { this.length = 2; } }
const n = Number(process.argv[2] ?? \"3\");
const x: any = n > 2 ? new W() : n > 1 ? Number : Math.max;
";

/// l1 (RED before): `length` of a builtin constructor and a builtin method
/// value is their own data property, a Number (20.2.4.1); the class arm
/// reads its own slot.
#[test]
fn r9read10_l1_builtin_fn_own_length() {
    let pf = facts(&format!("{LEN}console.log(x.length);"));
    assert!(proven(&sites(&pf, "length")));
}

/// l1b (RED before): builtin function arms only.
#[test]
fn r9read10_l1b_builtin_fn_only_length() {
    let pf = facts(
        "const n = Number(process.argv[2] ?? \"3\");
const x: any = n > 1 ? Number : Math.max;
console.log(x.length);",
    );
    assert!(proven(&sites(&pf, "length")));
}

/// KEEP l2: `delete` of the configurable own `length`.
#[test]
fn r9read10_l2_delete_length_kept() {
    let pf = facts(&format!(
        "{LEN}delete (Math.max as any).length;\nconsole.log(x.length);"
    ));
    assert!(!proven(&sites(&pf, "length")));
}

/// KEEP l3: `Object.defineProperty` replaces it by an accessor.
#[test]
fn r9read10_l3_define_getter_kept() {
    let pf = facts(&format!(
        "{LEN}Object.defineProperty(Math.max, \"length\", {{ get() {{ return 9; }} }});\nconsole.log(x.length);"
    ));
    assert!(!proven(&sites(&pf, "length")));
}

/// KEEP l4: an unknown-key delete reaching the function.
#[test]
fn r9read10_l4_unknown_key_delete_kept() {
    let pf = facts(&format!(
        "{LEN}delete x[process.argv[3] ?? \"q\"];\nconsole.log(x.length);"
    ));
    assert!(!proven(&sites(&pf, "length")));
}

/// KEEP l5: a store of `length` on the function (non-writable: it changes
/// nothing, but the model refuses any store that reaches the key).
#[test]
fn r9read10_l5_store_length_kept() {
    let pf = facts(&format!(
        "{LEN}(Number as any).length = 5;\nconsole.log(x.length);"
    ));
    assert!(!proven(&sites(&pf, "length")));
}

/// b1: `BYTES_PER_ELEMENT` of a typed-array constructor (23.2.6.1).
#[test]
fn r9read10_b1_bytes_per_element() {
    let pf = facts(
        "function convertArray(array: any, type: any) {
  if (!array || array.constructor === type) return array;
  if (typeof type.BYTES_PER_ELEMENT === \"number\") return new type(array);
  return Array.prototype.slice.call(array);
}
console.log(convertArray([1, 2], Float32Array).length);",
    );
    assert!(proven(&sites(&pf, "BYTES_PER_ELEMENT")));
}

/// b2 (RED before): the receiver may also be `undefined` (a call without
/// the argument): that arm throws TypeError (7.1.18 ToObject), the typed
/// array constructor arm reads its constant (23.2.6.1).
#[test]
fn r9read10_b2_bytes_per_element_or_undefined() {
    let pf = facts("function convertArray(array: any, type?: any) {
  if (!array || array.constructor === type) return array;
  if (typeof type.BYTES_PER_ELEMENT === \"number\") return new type(array);
  return Array.prototype.slice.call(array);
}
const n = Number(process.argv[2] ?? \"1\");
try { console.log(n > 0 ? convertArray([1, 2], Float32Array).length : convertArray([1, 2]).length); } catch (e) { console.log(\"TypeError\"); }");
    let v = sites(&pf, "BYTES_PER_ELEMENT");
    assert!(
        proven(&v) && v.iter().all(|x| x.starts_with("PerKind")),
        "{v:?}"
    );
}

/// b3: an unknown-key store on the typed array constructor cannot change
/// `BYTES_PER_ELEMENT` ({ [[Writable]]: false, [[Configurable]]: false },
/// 23.2.6.1: OrdinarySet and OrdinaryDefineOwnProperty both fail): still
/// decided.
#[test]
fn r9read10_b3_unknown_store_cannot_change_constant() {
    let pf = facts("function convertArray(array: any, type?: any) {
  if (typeof type.BYTES_PER_ELEMENT === \"number\") return new type(array);
  return array;
}
(Float32Array as any)[process.argv[3] ?? \"q\"] = 1;
const n = Number(process.argv[2] ?? \"1\");
try { console.log(n > 0 ? convertArray([1, 2], Float32Array).length : convertArray([1, 2]).length); } catch (e) { console.log(\"TypeError\"); }");
    let v = sites(&pf, "BYTES_PER_ELEMENT");
    assert!(proven(&v), "{v:?}");
}
