//! A spread (`{...s}`) or `Object.assign(t, s)` copies the source's own
//! enumerable properties (ECMA-262 CopyDataProperties, `Object.assign`):
//! their values, in key order, running the source's own getters. A builtin
//! object whose template is a container the model fills
//! (`Model::own_props_modelled`: `JSON.parse` values, `Object.keys` arrays,
//! `Object.groupBy` results …) holds every such value under its any key and no
//! accessor the runtime made, so the copy takes those values, never ⊤.
//! Every other builtin object keeps its own properties out of the model:
//! the copy stays ⊤.
//!
//! Each fixture calls whatever the copy holds under `cb` with `obj`: a ⊤
//! there is called with it, so `obj` escapes and the call `obj.len()` after
//! it is unproven. The read `o.a` of the copy's own key is proven where
//! the target's layout holds it (`Object.assign`), with or without ⊤ there.

use super::call_shadow::{call_of, proven};
use super::*;

const TEXT: &str = r#"'{"b":{"c":[1,"s",null]},"n":2}'"#;

fn fixture(source: &str, copy: &str) -> String {
    format!(
        r#"
class V {{ k = 1; len() {{ return this.k; }} }}
const obj = new V();
const src: any = {source};
const o: any = {copy};
const f = o.cb;
if (typeof f === "function") f(obj);
const r = o.a;
console.log(obj.len(), typeof r);
"#
    )
}

/// (the read `o.a` proven, the call `obj.len()` proven)
fn verdicts(source: &str, copy: &str) -> (bool, bool) {
    verdicts_of(&fixture(source, copy))
}

/// The same with an unseen module loaded: its code may supply the global the
/// source reads.
fn verdicts_unseen(source: &str, copy: &str) -> (bool, bool) {
    verdicts_of(&unseen(&fixture(source, copy)))
}

fn verdicts_of(program: &str) -> (bool, bool) {
    let pf = facts(program);
    (
        proven(&site(&pf, "a").verdict),
        proven(&call_of(&pf, "len").verdict),
    )
}

fn json() -> String {
    format!("JSON.parse({TEXT})")
}

#[test]
fn a_spread_of_json_data_copies_its_values() {
    assert!(verdicts(&json(), "{ a: obj, ...src }").1);
}

#[test]
fn object_assign_of_json_data_copies_its_values() {
    assert!(verdicts(&json(), "Object.assign({ a: obj }, src)").1);
}

/// `Object.keys` (and `Reflect.ownKeys`) arrays: the keys, Strings.
#[test]
fn a_spread_of_an_own_keys_array_copies_its_values() {
    assert!(verdicts("Object.keys(obj)", "{ a: obj, ...src }").1);
}

/// Sabotage twin: a Date's own properties are not in the model (nor a
/// Map's, a typed array's …): the copy stays ⊤.
#[test]
fn a_spread_of_an_unmodelled_builtin_object_stays_unproven() {
    assert!(!verdicts("new Date()", "{ a: obj, ...src }").1);
    assert!(!verdicts("new Map()", "Object.assign({ a: obj }, src)").1);
}

/// Sabotage twin: a value from unknown code.
#[test]
fn a_spread_of_an_unknown_value_stays_unproven() {
    let top = "(globalThis as any).ext";
    assert!(!verdicts_unseen(top, "{ a: obj, ...src }").1);
    assert!(!verdicts_unseen(top, "Object.assign({ a: obj }, src)").1);
}

/// Sabotage twin: the parsed value is handed to unknown code first, which
/// may store a function under `cb`: the copy carries that unknown value.
#[test]
fn json_data_handed_to_unknown_code_stays_unproven() {
    let source = format!(
        "((x: any) => {{ (globalThis as any).hook(x); return x; }})({})",
        json()
    );
    assert!(!verdicts(&source, "{ a: obj, ...src }").1);
}

/// Sabotage twin: a program getter on the source runs during the copy and
/// its result (here from unknown code) is copied.
#[test]
fn a_program_getter_result_is_copied() {
    let unknown = "{ get cb() { return (globalThis as any).g; } }";
    assert!(!verdicts_unseen(unknown, "{ a: obj, ...src }").1);
    assert!(!verdicts_unseen(unknown, "Object.assign({ a: obj }, src)").1);
}

/// Twin: the parsed data may hold any key, `b` among them; the copy's `b`
/// is `obj` or a parsed value, so `o.b.len()` is no call of `V::len` only.
#[test]
fn a_key_the_parsed_data_may_supply_reads_the_parsed_value() {
    let src = format!(
        r#"
class V {{ k = 1; len() {{ return this.k; }} }}
const obj = new V();
const cfg: any = {};
const o: any = {{ b: obj, ...cfg }};
console.log(o.b.len());
"#,
        json()
    );
    let pf = facts(&src);
    assert!(!proven(&call_of(&pf, "len").verdict));
}

/// Sabotage twin: the program writes into the parsed value through a
/// computed key, here a value from unknown code: the write lands in the
/// source's any key and the copy carries it.
#[test]
fn a_computed_key_write_into_json_data_is_copied() {
    let source = format!(
        "((x: any) => {{ x[String(Math.random())] = (globalThis as any).g; return x; }})({})",
        json()
    );
    assert!(!verdicts_unseen(&source, "{ a: obj, ...src }").1);
    assert!(!verdicts_unseen(&source, "Object.assign({ a: obj }, src)").1);
}

/// Sabotage twin: a primitive method's container (`"ab".match("a")`, an
/// undescribed result) is born with an empty any key; its strings come from
/// the load rule, not its fields. The copy's `o[0]` is a String at run
/// time, so `(o[0] || obj).len()` is no call of `V::len` only.
#[test]
fn a_spread_of_a_primitive_method_result_keeps_its_strings() {
    let src = r#"
class V { k = 1; len() { return this.k; } }
const obj = new V();
const s: any = "ab".match("a");
const o: any = { ...s };
const v = o[0] || obj;
console.log(v.len());
"#;
    let pf = facts(src);
    assert!(!proven(&call_of(&pf, "len").verdict));
}

/// A namespace or constructor of the realm has no enumerable own property
/// (ECMA-262 18): a spread or `Object.assign` of it copies only what the
/// program stored on it, here nothing, so the copy's `cb` is no unknown
/// function and `obj` stays the program's.
#[test]
fn a_spread_of_a_realm_namespace_copies_no_unknown_value() {
    for ns in ["Object", "Math", "JSON", "Map", "Object.prototype.hasOwnProperty"] {
        assert!(verdicts(ns, "{ a: obj, ...src }").1, "spread of {ns}");
        assert!(verdicts(ns, "Object.assign({ a: obj }, src)").1, "assign of {ns}");
    }
}

/// What the program stored on a realm namespace is copied, under its key.
#[test]
fn a_program_store_on_a_realm_namespace_is_copied() {
    let src = r#"
class V { k = 1; len() { return this.k; } }
const obj = new V();
(Object as any).cb = (x: any) => x.len();
const o: any = { a: obj, ...(Object as any) };
const f = o.cb;
if (typeof f === "function") f(obj);
console.log(obj.len(), typeof o.a);
"#;
    let pf = facts(src);
    assert!(proven(&call_of(&pf, "len").verdict));
}

/// Sabotage twin: an unknown value stored on the namespace is copied, and
/// the copy calls it with `obj`.
#[test]
fn an_unknown_value_stored_on_a_realm_namespace_stays_unproven() {
    let program = unseen(
        r#"
class V { k = 1; len() { return this.k; } }
const obj = new V();
(Object as any).cb = (globalThis as any).ext;
const o: any = { a: obj, ...(Object as any) };
const f = o.cb;
if (typeof f === "function") f(obj);
console.log(obj.len(), typeof o.a);
"#,
    );
    let pf = facts(&program);
    assert!(!proven(&call_of(&pf, "len").verdict));
}

/// Controls: `Error` has the enumerable `stackTraceLimit` (V8), `console`
/// and `process` are host objects with enumerable own members: the copy
/// stays unknown.
#[test]
fn a_spread_of_error_or_a_host_namespace_stays_unproven() {
    for ns in ["Error", "console", "process"] {
        assert!(!verdicts(ns, "{ a: obj, ...src }").1, "spread of {ns}");
    }
}

/// A function the realm gives a namespace or prototype has no enumerable
/// own property (ECMA-262 10.3.4 CreateBuiltinFunction; 10.2.9, 10.2.10:
/// `name` and `length` non-enumerable), a RegExp object only the
/// non-enumerable `lastIndex` (22.2.3.1), a Promise none (27.2.4.7): a copy
/// of one copies only what the program stored on it, here nothing.
#[test]
fn a_copy_of_a_realm_function_or_object_copies_no_unknown_value() {
    for src in ["[].map", "Array.prototype.sort", "Object.keys", "/a/g", "Promise.resolve(1)"] {
        assert!(verdicts(src, "{ a: obj, ...src }").1, "spread of {src}");
        assert!(verdicts(src, "Object.assign({ a: obj }, src)").1, "assign of {src}");
    }
}

/// Twin: what the program stored on a realm function is copied, under its key.
#[test]
fn a_program_store_on_a_realm_function_is_copied() {
    let src = r#"
class V { k = 1; len() { return this.k; } }
const obj = new V();
const m: any = [].map;
m.cb = (x: any) => x.len();
const o: any = { a: obj, ...m };
const f = o.cb;
if (typeof f === "function") f(obj);
console.log(obj.len(), typeof o.a);
"#;
    let pf = facts(src);
    assert!(proven(&call_of(&pf, "len").verdict));
}

/// Control: the realm function is handed to unknown code first, which may
/// store a function under `cb`: the copy carries that unknown value.
#[test]
fn a_realm_function_handed_to_unknown_code_stays_unproven() {
    for src in ["[].map", "/a/g"] {
        let source =
            format!("((x: any) => {{ new Function(process.argv[2])(x); return x; }})({src})");
        assert!(!verdicts(&source, "{ a: obj, ...src }").1, "spread of {src}");
        assert!(!verdicts(&source, "Object.assign({ a: obj }, src)").1, "assign of {src}");
    }
}
