//! A built-in that reads or writes a program object by key runs the
//! accessors under that key: [[Get]] runs the getter and [[Set]] the setter
//! found on the object or its chain, with the object as `this`.
//! Object.assign is [[Set]] on its target under every source key, named or
//! not (ECMA-262 20.1.2.1); bind reads the target's `length` and `name`
//! (20.2.3.2), as its runtime declaration says (`reads(field(this, k))`).
//! JSON.stringify [[Get]]s each own enumerable key of what it serializes
//! (25.5.2.5), so an accessor defined on a `JSON.parse` record after its
//! birth runs. CXCHK0 gap witnesses test_gap_11866_object_assign_inherited,
//! test_gap_bind_creation_metadata, test_gap_call_bind_explicit_this,
//! test_gap_json_root_record.

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

#[test]
fn json_stringify_reads_own_getters_recursively() {
    runs(
        "own getter of a parsed record",
        r#"
function hit() { return 7; }
const row: any = JSON.parse("{\"a\":1}");
Object.defineProperty(row, "score", { get() { return hit(); }, enumerable: true });
console.log(JSON.stringify(row));
"#,
    );
    runs(
        "nested literal getter",
        r#"
function hit() { return 7; }
const row: any = { child: { get g() { return hit(); } } };
console.log(JSON.stringify(row));
"#,
    );
    stays(
        "not stringified",
        r#"
function hit() { return 7; }
const row: any = { get g() { return hit(); } };
const other = { a: 1 };
console.log(JSON.stringify(other), typeof row);
"#,
    );
    stays(
        "getter on another parsed record",
        r#"
function hit() { return 7; }
const row: any = JSON.parse("{\"a\":1}");
Object.defineProperty(row, "score", { get() { return hit(); }, enumerable: true });
const other: any = JSON.parse("{\"b\":2}");
console.log(JSON.stringify(other), typeof row);
"#,
    );
}

/// A literal getter on an object given as a prototype by value runs when a
/// read reaches it through the chain, whichever the analysis meets first:
/// the read of the child or the literal that defines the getter.
#[test]
fn literal_getter_on_a_given_prototype_runs() {
    runs(
        "Object.create",
        r#"
function hit() { return 1; }
const p = { get k() { return hit(); } };
const c = Object.create(p);
console.log(c.k);
"#,
    );
    runs(
        "setPrototypeOf",
        r#"
function hit() { return 1; }
const p = { get k() { return hit(); } };
const c: any = {};
Object.setPrototypeOf(c, p);
console.log(c.k);
"#,
    );
    runs(
        "method call through the getter",
        r#"
function hit() { return 1; }
const p = { get m() { hit(); return function () { return 2; }; } };
const c = Object.create(p);
console.log(c.m());
"#,
    );
    runs(
        "inherited toJSON getter",
        r#"
function hit() { return 1; }
const p = { get toJSON() { hit(); return function (this: any) { return this.answer; }; } };
const c: any = Object.create(p);
c.answer = 83;
console.log(JSON.stringify(c));
"#,
    );
    stays(
        "another key of the prototype",
        r#"
function hit() { return 1; }
const p = { get k() { return hit(); }, get j() { return 2; } };
const c = Object.create(p);
console.log(c.j);
"#,
    );
    stays(
        "a getter literal that is no prototype of the read",
        r#"
function hit() { return 1; }
const p = { get k() { return hit(); } };
const c = Object.create({ j: 1 });
console.log(c.k, typeof p);
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
