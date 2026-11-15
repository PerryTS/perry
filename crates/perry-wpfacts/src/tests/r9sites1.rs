//! R9SITES1 fixtures: JSON.stringify (ECMA-262 25.5.2.2
//! SerializeJSONProperty) looks up `toJSON` on the value (its chain) and,
//! for a non-callable object, serializes its ENUMERABLE OWN properties
//! (25.5.2.5 SerializeJSONObject, 25.5.2.6 SerializeJSONArray via
//! EnumerableOwnProperties 7.3.23); a callable value serializes as
//! `undefined` and is never descended (step 10). r9: the any-key read of
//! the JSON touch walked each value chain (methods, `constructor`), then the
//! class `prototype`, and called every `toJSON` with `this` = a class
//! prototype object (three.core.js `Object3D.toJSON`, `Material.toJSON`).
use super::*;

/// The receivers of the reads of `key` (no calls), `proto:` kept.
fn recv_p(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| {
            let last = r.rsplit(':').next().unwrap_or(r).to_string();
            if r.starts_with("proto:") { format!("proto:{last}") } else { last }
        })
        .collect();
    r.sort();
    r.dedup();
    r
}

const CLS: &str = "class Base { kind: any; constructor() { this.kind = 1; } toJSON() { return this.tk; } }
class Sub extends Base { tk: any; constructor() { super(); this.tk = { z: 2 }; } }
class A { tj: any; v: any; constructor() { this.v = Number( process.argv.length ); this.tj = { q: 1 }; } m() { return this.v; } toJSON() { return this.tj; } }
class B { tb: any; constructor() { this.tb = String( Math.random() ); } n() { return 1; } toJSON() { return this.tb; } }
";

// J1 RED: an instance in a console argument (the formatter, `%j` JSON): `toJSON` runs on it only,
// never on `A.prototype` (reached through `constructor.prototype`).
#[test]
fn r9sites1_j1_tojson_this_is_the_value() {
    let pf = facts(&format!("{CLS}const a = new A();\nconsole.log( String( Math.random() ), a.m(), {{ a: a }} );\n"));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES1 J1 tj recv={r:?}");
    assert_eq!(r, vec!["A".to_string()], "J1 toJSON this: {r:?}");
}

// J2 RED: a class value inside a console argument is callable: never
// descended, so its prototype is not serialized either.
#[test]
fn r9sites1_j2_callable_not_descended() {
    let pf = facts(&format!("{CLS}const a = new A();\nconsole.log( String( Math.random() ), a.m(), {{ k: A, n: 1 }} );\n"));
    let r = recv_p(&pf, "tj");
    eprintln!("R9SITES1 J2 tj recv={r:?}");
    assert!(!r.iter().any(|x| x.starts_with("proto:")), "J2 class value descended: {r:?}");
}

// K1 KEEP: an own value (nested, array element) is serialized: its toJSON runs.
#[test]
fn r9sites1_k1_own_values_serialized() {
    let pf = facts(&format!("{CLS}const b = new B();\nconsole.log( b.n(), JSON.stringify( {{ o: {{ x: [ b ] }} }} ) );\n"));
    let r = recv_p(&pf, "tb");
    eprintln!("R9SITES1 K1 tb recv={r:?}");
    assert!(r.contains(&"B".to_string()), "K1 nested toJSON lost: {r:?}");
}

// K2 KEEP: an own getter of a literal runs (25.5.2.5 step 8.a Get): its
// result is serialized (its toJSON runs).
#[test]
fn r9sites1_k2_own_getter_runs() {
    let pf = facts(&format!("{CLS}const b = new B();\nconst o = {{ get g() {{ return b; }} }};\nconsole.log( b.n(), JSON.stringify( o ) );\n"));
    let r = recv_p(&pf, "tb");
    eprintln!("R9SITES1 K2 tb recv={r:?}");
    assert!(r.contains(&"B".to_string()), "K2 getter result lost: {r:?}");
}

// K3 KEEP: a console argument is serialized by `%j`: a nested own value runs its toJSON.
#[test]
fn r9sites1_k3_console_json_nested() {
    let pf = facts(&format!("{CLS}const b = new B();\nconsole.log( String( Math.random() ), b.n(), {{ o: [ b ] }} );\n"));
    let r = recv_p(&pf, "tb");
    eprintln!("R9SITES1 K3 tb recv={r:?}");
    assert!(r.contains(&"B".to_string()), "K3 console nested toJSON lost: {r:?}");
}

// J3 RED (r9 shape: three.core.js Object3D.toJSON, Mesh extends Object3D,
// `console.warn( message, object )`): a subclass instance as a console
// argument; the inherited toJSON runs on the instance only.
#[test]
fn r9sites1_j3_inherited_tojson_console_instance() {
    let pf = facts(&format!("{CLS}const s = new Sub();\nconsole.log( String( Math.random() ), s );\n"));
    let r = recv_p(&pf, "tk");
    eprintln!("R9SITES1 J3 tk recv={r:?}");
    assert_eq!(r, vec!["Sub".to_string()], "J3 toJSON this: {r:?}");
}
