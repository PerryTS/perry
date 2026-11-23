// R9BIRTH1 (2026-11-15): RED fixtures for "key not a definite birth field"
// on sites three.js loses class-wide (r9 census items 2 and 4), small JS:
// A: a computed-key store whose key has a known shape counted as a writer of
//    EVERY key of the class (`writers_post`), so a method call on the class
//    (`scale.set( 1, 1, 1 )`) found "a written own key" for the method name;
// B: a `switch` whose cases each start where the cases above left the
//    layout order, also after a `break` (PropertyMixer's constructor);
// C: a subclass constructor storing an inherited key after `super()`
//    (`this.type = 'MeshStandardMaterial'`) counted as an out-of-order add.
// Proven = a verdict that is neither No(..) nor Throws.
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

/// (offset, is_call, is_store, verdict) of every site of `key` in a.ts.
fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<(u32, bool, bool, crate::Verdict)> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key)
        .map(|s| {
            (
                s.byte_offset,
                s.is_call,
                s.access.is_store(),
                s.verdict.clone(),
            )
        })
        .collect()
}

/// Proven by the construction-phase layout. R9STORE4: a known-object
/// store (rule A: the store adds or sets an own data property, exact for
/// any layout) is no layout claim, so these KEEP tests do not count it.
fn proven(v: &crate::Verdict) -> bool {
    !matches!(
        v,
        crate::Verdict::No(_) | crate::Verdict::Throws | crate::Verdict::KnownObjectStore
    )
}

fn show(tag: &str, pf: &crate::ProgramFacts, keys: &[&str], class: &str) {
    for k in keys {
        for s in sites(pf, k) {
            eprintln!(
                "RB1 {tag} .{k} @{} call={} store={} verdict={:?}",
                s.0, s.1, s.2, s.3
            );
        }
    }
    for t in pf.templates.iter().filter(|t| t.display.ends_with(class)) {
        eprintln!(
            "RB1 {tag} tmpl {} fixed_layout={} adds_keys={} birth_keys={:?}",
            t.display, t.fixed_layout, t.adds_keys, t.birth_keys
        );
        for f in &t.fields {
            eprintln!(
                "RB1 {tag}   field {} slot={:?} definite={} wc={} wp={}",
                f.key, f.slot, f.definite, f.writers_ctor, f.writers_post
            );
        }
    }
}

/// Every site of `key` (of the given kind) is proven / every one is not.
fn all(pf: &crate::ProgramFacts, key: &str, call: bool, store: bool, want: bool) -> bool {
    let s: Vec<_> = sites(pf, key)
        .into_iter()
        .filter(|s| s.1 == call && s.2 == store)
        .collect();
    !s.is_empty() && s.iter().all(|s| proven(&s.3) == want)
}

// The computed-key store of three.js PropertyBinding (`targetObject[
// propertyName ] = v`): the key is a field holding names from a known set.
const A_HEAD: &str = "class V { constructor(x: number) { this.x = x; this.y = 0; }
  set(a: number, b: number) { this.x = a; this.y = b; return this; }
  len() { return this.x + this.y; } }
class B { constructor(o: any, name: string) { this.o = o; this.name = name; }
  put(val: number) { this.o[ this.name ] = val; } }
";

fn a_src(names: &str) -> String {
    format!(
        "{A_HEAD}const names = [{names}];
const v = new V(Number(process.argv[2] ?? \"1\"));
const b = new B(v, names[Number(process.argv[2] ?? \"0\") % names.length]);
b.put(5);
v.set(1, 2);
console.log(v.len());
"
    )
}

// A1: the computed key may be "x" or "w": it may add "w", never "set" or
// "len" (OrdinarySet creates only property P, ECMA-262 10.1.9.2): the method
// calls are judged on the prototype (10.1.8.1).
#[test]
fn r9birth1_a1_shaped_key_store_leaves_methods() {
    let pf = facts(&a_src("\"x\", \"w\""));
    show("A1", &pf, &["set", "len"], "V");
    assert!(
        all(&pf, "set", true, false, true),
        "A1 .set call not proven"
    );
    assert!(
        all(&pf, "len", true, false, true),
        "A1 .len call not proven"
    );
}

// A2 KEEP: the key may be "set": an own `set` may shadow the method.
#[test]
fn r9birth1_a2_key_may_name_the_method() {
    let pf = facts(&a_src("\"x\", \"set\""));
    show("A2", &pf, &["set", "len"], "V");
    assert!(
        all(&pf, "set", true, false, false),
        "A2 .set call proven, but the key may be \"set\""
    );
    assert!(
        all(&pf, "len", true, false, true),
        "A2 .len call not proven"
    );
}

// A3 KEEP: a key of unknown content may be any key.
#[test]
fn r9birth1_a3_unknown_key() {
    let src = format!(
        "{A_HEAD}const v = new V(1);
const b = new B(v, String(process.argv[2]));
b.put(5);
v.set(1, 2);
console.log(v.len());
"
    );
    let pf = facts(&src);
    show("A3", &pf, &["set"], "V");
    assert!(
        all(&pf, "set", true, false, false),
        "A3 .set call proven under an unknown key"
    );
}

// B1: PropertyMixer's constructor. Each case ends in `break` (or is the
// last); the stores after the switch run on every path in layout order.
const B1: &str = "class M { constructor(t: string, n: number) {
    this.n = n;
    switch (t) {
      case \"q\": this.buf = new Float64Array(n * 6); this.wi = 5; break;
      case \"s\":
      case \"b\": this.buf = new Array(n * 5); break;
      default: this.buf = new Float64Array(n * 5);
    }
    this.mix = 1; this.ci = 0; }
  get2() { return this.mix + this.ci + this.buf.length; } }
const m = new M(process.argv[2] ?? \"q\", 2);
console.log(m.get2());
";

#[test]
fn r9birth1_b1_switch_cases_start_at_the_dispatch() {
    let pf = facts(B1);
    show("B1", &pf, &["mix", "ci", "buf", "wi"], "M");
    assert!(
        all(&pf, "mix", false, true, true),
        "B1 this.mix store not proven"
    );
    assert!(
        all(&pf, "ci", false, true, true),
        "B1 this.ci store not proven"
    );
    assert!(
        all(&pf, "buf", false, true, true),
        "B1 this.buf stores not proven"
    );
    assert!(
        all(&pf, "mix", false, false, true),
        "B1 this.mix read not proven"
    );
}

// B2 KEEP: case 1 falls through into case 2: run 1 adds `b` before `a`,
// which the layout has first: an out-of-order add, and every key after it.
const B2: &str = "class N { constructor(t: number) {
    switch (t) {
      case 1: this.b = 1;
      case 2: this.a = 2; break;
      default: this.a = 3; this.b = 4;
    }
    this.c = 5; }
  get() { return this.c; } }
const n = new N(Number(process.argv[2] ?? \"1\"));
console.log(n.get());
";

#[test]
fn r9birth1_b2_fall_through_out_of_order() {
    let pf = facts(B2);
    show("B2", &pf, &["a", "b", "c"], "N");
    assert!(
        all(&pf, "c", false, true, false),
        "B2 this.c store proven after an out-of-order add"
    );
}

// C1: MeshStandardMaterial's constructor: `super()`, then a store of the
// parent's key `type` (own data from the parent's construction: no add),
// then the class's own keys.
const C1: &str = "class P { constructor() { this.type = \"P\"; this.k = 1; } }
class Q extends P { constructor() { super(); this.type = \"Q\"; this.d = { a: 1 }; this.c = 2; }
  m() { return this.c + this.d.a + this.k; } }
const q = new Q();
console.log(q.m(), q.type, String(Math.random()).length > 0);
";

#[test]
fn r9birth1_c1_inherited_key_store_is_no_add() {
    let pf = facts(C1);
    show("C1", &pf, &["type", "d", "c"], "Q");
    assert!(
        all(&pf, "d", false, true, true),
        "C1 this.d store not proven"
    );
    assert!(
        all(&pf, "c", false, true, true),
        "C1 this.c store not proven"
    );
    assert!(
        all(&pf, "c", false, false, true),
        "C1 this.c read not proven"
    );
}

// C2 KEEP: the parent constructor may return another object: `this` after
// `super()` need not hold `type` (10.2.2 step 10).
const C2: &str = "class P { constructor() { this.type = \"P\"; if (process.argv.length > 99) { return { type: \"o\" }; } } }
class Q extends P { constructor() { super(); this.type = \"Q\"; this.c = 2; }
  m() { return this.c; } }
const q = new Q();
console.log(q.m());
";

#[test]
fn r9birth1_c2_parent_returns_an_object() {
    let pf = facts(C2);
    show("C2", &pf, &["type", "c"], "Q");
    assert!(
        all(&pf, "c", false, true, false),
        "C2 this.c store proven although super() may return another object"
    );
}

// C3 KEEP: `delete this.type` removes the inherited key: the store after it
// adds the key again, at the end (10.1.10, 10.1.9.2).
const C3: &str = "class P { constructor() { this.type = \"P\"; } }
class Q extends P { constructor() { super(); delete this.type; this.type = \"Q\"; this.c = 2; }
  m() { return this.c; } }
const q = new Q();
console.log(q.m(), Object.keys(q).join());
";

#[test]
fn r9birth1_c3_deleted_inherited_key() {
    let pf = facts(C3);
    show("C3", &pf, &["type", "c"], "Q");
    assert!(
        all(&pf, "c", false, true, false),
        "C3 this.c store proven after a delete of an inherited key"
    );
}
