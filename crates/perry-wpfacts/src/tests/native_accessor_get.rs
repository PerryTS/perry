//! A built-in that reads or writes a program object by key runs the
//! accessors under that key: [[Get]] runs the getter and [[Set]] the setter
//! found on the object or its chain, with the object as `this`.
//! Object.assign is [[Set]] on its target under every source key, named or
//! not (ECMA-262 20.1.2.1); bind reads the target's `length` and `name`
//! (20.2.3.2), as its runtime declaration says (`reads(field(this, k))`).
//! CXCHK0 gap witnesses test_gap_11866_object_assign_inherited,
//! test_gap_bind_creation_metadata, test_gap_call_bind_explicit_this.

use super::*;

/// Whether `hit` is an active body in `src`.
fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

fn runs(name: &str, src: &str) {
    assert!(hit_runs(src), "{name}: hit inactive, Node runs it");
}

fn stays(name: &str, src: &str) {
    assert!(!hit_runs(src), "{name}: hit active, Node never runs it");
}

#[test]
fn object_assign_runs_the_target_chain_setter() {
    runs(
        "inherited setter",
        r#"
function hit(v: any) { return v; }
const proto = { set k(v: any) { hit(v); } };
const target = Object.create(proto);
Object.assign(target, { k: 1 });
console.log(Object.prototype.hasOwnProperty.call(target, "k"));
"#,
    );
    runs(
        "own setter, value call",
        r#"
function hit(v: any) { return v; }
const target = { set k(v: any) { hit(v); } };
const assign: any = [Object.assign][0];
assign(target, { k: 2 });
console.log(typeof target);
"#,
    );
}

#[test]
fn object_assign_runs_a_computed_key_setter() {
    runs(
        "symbol setter",
        r#"
function hit(v: any) { return v; }
const symbol = Symbol("key");
const symbolProto: any = {};
Object.defineProperty(symbolProto, symbol, { set(v: any) { hit(v); } });
const target = Object.create(symbolProto);
const source: any = {};
source[symbol] = 8;
Object.assign(target, source);
console.log(Object.prototype.hasOwnProperty.call(target, symbol));
"#,
    );
}

#[test]
fn object_assign_and_spread_controls() {
    stays(
        "other key",
        r#"
function hit(v: any) { return v; }
const proto = { set k(v: any) { hit(v); } };
const target = Object.create(proto);
Object.assign(target, { j: 1 });
console.log(Object.keys(target).length);
"#,
    );
    stays(
        "object spread defines",
        r#"
function hit(v: any) { return v; }
const proto = { set k(v: any) { hit(v); } };
const src = Object.create(proto);
const copy = { ...{ k: 1 }, ...src };
console.log(copy.k);
"#,
    );
}

#[test]
fn bind_reads_the_target_length_and_name() {
    runs(
        "length getter",
        r#"
function hit() { return 3; }
function f(a: any, b: any) { return a + b; }
Object.defineProperty(f, "length", { get() { return hit(); }, configurable: true });
const g = f.bind(null, 1);
console.log(g.length);
"#,
    );
    runs(
        "name getter",
        r#"
function hit() { return "n"; }
function f(a: any) { return a; }
Object.defineProperty(f, "name", { get() { return hit(); }, configurable: true });
const g = f.bind({});
console.log(g.name);
"#,
    );
    stays(
        "other key",
        r#"
function hit() { return 3; }
function f(a: any) { return a; }
Object.defineProperty(f, "other", { get() { return hit(); }, configurable: true });
const g = f.bind(null);
console.log(g(1));
"#,
    );
}

/// structuredClone serializes its argument (StructuredSerialize, HTML
/// 2.7.3): it [[Get]]s every own value, so own getters run, nested ones
/// too, and it calls nothing else (a function throws DataCloneError).
#[test]
fn structured_clone_reads_own_values_and_calls_nothing() {
    runs(
        "own getter",
        r#"
function hit() { return 1; }
const o = { get g() { return hit(); } };
console.log(typeof structuredClone(o));
"#,
    );
    runs(
        "nested own getter",
        r#"
function hit() { return 1; }
const parent: any = { first: {}, tail: 2 };
Object.defineProperty(parent.first, "trigger", { enumerable: true, get() { return hit(); } });
console.log(typeof structuredClone(parent));
"#,
    );
    stays(
        "a method of the clone source",
        r#"
function hit() { return 1; }
const o = { a: 1, m() { return hit(); } };
try { structuredClone(o); } catch (e) { console.log("DataCloneError"); }
console.log(typeof o);
"#,
    );
    stays(
        "a getter of an object that is not cloned",
        r#"
function hit() { return 1; }
const other = { get g() { return hit(); } };
console.log(typeof structuredClone({ a: 1 }), typeof other);
"#,
    );
}
