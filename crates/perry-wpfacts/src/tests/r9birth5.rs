// R9BIRTH5 (2026-11-15): rule B3 without a fixed layout. A read of key K
// through a program class instance finds, by OrdinaryGet (ECMA-262
// 10.1.8.1), the first object on the chain that HAS K as its own property;
// keys other than K that the instance gains after birth do not change that
// object. So when K can never be an own key of the instance (no named,
// computed, `Object.assign` or `defineProperty` store may give it K; K is no
// birth key) and every chain link below the holder lacks K and cannot get
// it, the read is the holder's constant prototype member, whatever the
// instance's layout. Several receiver classes with different holders give a
// set of members (T0Set).
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

/// The verdicts (Debug text) and receivers of the read/call sites of `key`.
fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<(String, Vec<String>)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| (format!("{:?}", s.verdict), s.receivers.clone()))
        .collect();
    for (verdict, r) in &v {
        eprintln!("RB5 .{key} read verdict={verdict} receivers={r:?}");
    }
    v
}

fn all(pf: &crate::ProgramFacts, key: &str, want: &str) -> bool {
    let r = reads(pf, key);
    !r.is_empty() && r.iter().all(|(v, _)| v == want)
}

fn none_proven(pf: &crate::ProgramFacts, key: &str) -> bool {
    let r = reads(pf, key);
    !r.is_empty() && r.iter().all(|(v, _)| v.starts_with("No"))
}

/// `bind` adds keys `y` and `f` after birth (the layout is not fixed), and
/// reads the method `_a` as a value. `extra` goes into the class body.
fn src(extra: &str, tail: &str) -> String {
    format!(
        "class P {{
  x: any;
  constructor(x: any) {{ this.x = x; }}
  _a() {{ return this.x + 1; }}
  bind() {{ (this as any).y = 1; (this as any).f = this._a; return (this as any).f(); }}
{extra}
}}
{tail}
const p = new P(process.argv[2] ?? \"1\");
console.log(p.bind(), (p as any).y);
"
    )
}

// B1 (RED at rb4): the instance gains other keys in a method; `_a` is
// never its own key: the read is P.prototype._a.
#[test]
fn r9birth5_b1_other_keys_added() {
    let pf = facts(&src("", ""));
    assert!(all(&pf, "_a", "T0Mono"), "B1 this._a not T0Mono");
}

// b2 (KEEP): a method stores `this._a`: own or prototype: unproven.
#[test]
fn r9birth5_b2_own_store_of_the_key() {
    let pf = facts(&src("  reset() { (this as any)._a = () => 3; }", "new P(1).reset();"));
    assert!(none_proven(&pf, "_a"), "b2 this._a proven although stored on the instance");
}

// b3 (KEEP): a computed-key store with an unknown key may name `_a`.
#[test]
fn r9birth5_b3_computed_store() {
    let pf = facts(&src(
        "  put(k: any) { (this as any)[k] = 5; }",
        "new P(1).put(process.argv[2]);",
    ));
    assert!(none_proven(&pf, "_a"), "b3 this._a proven beside an unknown computed store");
}

// b4 (KEEP): `Object.assign(this, o)` with an unknown `o` may add `_a`.
#[test]
fn r9birth5_b4_object_assign() {
    let pf = facts(&src(
        "  merge(o: any) { Object.assign(this, o); }",
        "new P(1).merge(JSON.parse(process.argv[2] ?? \"{}\"));",
    ));
    assert!(none_proven(&pf, "_a"), "b4 this._a proven beside Object.assign of unknown");
}

// b5 (KEEP): `Object.defineProperty(this, '_a', ..)` adds an own `_a`.
#[test]
fn r9birth5_b5_define_property() {
    let pf = facts(&src(
        "  def() { Object.defineProperty(this, '_a', { value: 7 }); }",
        "new P(1).def();",
    ));
    assert!(none_proven(&pf, "_a"), "b5 this._a proven beside defineProperty");
}

// B6: a subclass prototype defines `_a` too; both classes reach the site:
// the read is one of two constant members, P's or Q's: a set (T0Set).
#[test]
fn r9birth5_b6_subclass_override_set() {
    let pf = facts(&src(
        "",
        "class Q extends P { _a() { return 9; } }\nconsole.log(new Q(String(Math.random())).bind());",
    ));
    let r = reads(&pf, "_a");
    assert!(
        r.iter().any(|(v, rs)| v == "T0Set" && rs.len() == 2),
        "B6 this._a not a set over P and Q: {r:?}"
    );
}

// Item 3: `this.constructor` read for `new this.constructor(..)`. Every
// class prototype has its own data property `constructor`, defined when
// the class is evaluated (ECMA-262 15.7.14 ClassDefinitionEvaluation:
// MakeConstructor, CreateMethodProperty(proto, "constructor", F)), before
// any instance can exist. Instances never own it unless a store gives it;
// `Symbol.species` is not involved (no ArraySpeciesCreate / SpeciesConstructor
// here: a plain [[Get]] of "constructor").
fn ctor_src(extra: &str, tail: &str) -> String {
    format!(
        "class V {{
  x: any;
  constructor(x: any) {{ this.x = x; }}
  clone() {{ return new (this as any).constructor(this.x); }}
  mark() {{ (this as any).m = 1; }}
{extra}
}}
class W extends V {{ }}
const v = new V(process.argv[2] ?? \"1\"); const w = new W(String(Math.random()));
v.mark(); w.mark();
console.log(v.clone().x, w.clone().x);
{tail}
"
    )
}

// C1 (RED at rb4): `mark` adds a key after birth (the layout is not
// fixed); the read is V.prototype.constructor or W.prototype.constructor.
#[test]
fn r9birth5_c1_constructor_set() {
    let pf = facts(&ctor_src("", ""));
    let r = reads(&pf, "constructor");
    assert!(
        !r.is_empty() && r.iter().all(|(v, _)| v == "T0Set" || v == "T0Mono"),
        "C1 this.constructor not proven: {r:?}"
    );
}

// c2 (KEEP): a store `this.constructor = X` on an instance.
#[test]
fn r9birth5_c2_constructor_stored() {
    let pf = facts(&ctor_src(
        "  re() { (this as any).constructor = Object; }",
        "if (process.argv.length > 5) new V(1).re();",
    ));
    let r = reads(&pf, "constructor");
    let src_reads: Vec<_> = r.iter().filter(|(v, _)| !v.starts_with("No")).collect();
    assert!(src_reads.is_empty(), "c2 this.constructor proven although stored: {r:?}");
}
