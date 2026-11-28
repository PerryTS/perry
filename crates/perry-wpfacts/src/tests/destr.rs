//! DESTR1 fixtures: rule DESTR-POS, array-pattern destructuring reads exact
//! positions.
//!
//! An array binding pattern over an ordinary array pulls position i with
//! the cursor equal to i (ECMA-262 8.6.2 IteratorBindingInitialization:
//! the array iterator's `next` reads index i on the i-th pull, and every
//! earlier pull either advanced or ended the pattern), so the element
//! bound at literal position i is the array's element i, not any element
//! of the array.
use super::*;

/// The receivers of the reads of `key` (no calls, no stores), last name part.
fn recv(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call && !s.access.is_store())
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

const KV: &str = "class K { kf = 1; } class V { vf = 2; } class W { wf = 3; }
const k = new K(); const v = new V(); const w = new W();
";

fn has(r: &[String], c: &str) -> bool {
    r.iter().any(|x| x == c)
}

// DESTR-POS RED (m2): a tuple-typed array local: p = t[0], q = t[1].
#[test]
#[ignore = "DESTR-LIT open: an array literal is not index-exact (no index slots), so its constant-index reads merge"]
fn destr_pos_tuple_local() {
    let pf = facts(&format!(
        "{KV}const t: [K, V] = [k, v];
const [p, q] = t;
console.log(q.vf, p.kf);
"
    ));
    let (kf, vf) = (recv(&pf, "kf"), recv(&pf, "vf"));
    eprintln!("DESTR tuple kf={kf:?} vf={vf:?}");
    assert!(!has(&kf, "V") && !has(&vf, "K"), "positions merged: kf={kf:?} vf={vf:?}");
}

// DESTR-POS RED (m2): `for (const [k2, v2] of map)`: the entry's position 0
// is the key, position 1 the value.
#[test]
fn destr_pos_map_for_of() {
    let pf = facts(&format!(
        "{KV}const mm = new Map<K, V>(); mm.set(k, v);
for (const [a, b] of mm) console.log(b.vf, a.kf);
"
    ));
    let (kf, vf) = (recv(&pf, "kf"), recv(&pf, "vf"));
    eprintln!("DESTR map kf={kf:?} vf={vf:?}");
    assert!(!has(&kf, "V") && !has(&vf, "K"), "positions merged: kf={kf:?} vf={vf:?}");
}

// DESTR-POS RED (m1): nested maps iterated in a method; the inner map's
// entries bind [name, r]: list holds no key of the outer map.
#[test]
fn destr_pos_nested_map_method() {
    let pf = facts(&format!(
        "{KV}class Anim {{
  tr = new Map<K, Map<string, V>>();
  apply(): number {{
    let s = 0;
    for (const [e, list] of this.tr) {{
      console.log(list.size, e.kf);
      for (const [name, r] of list) {{ s += name.length + r.vf; }}
    }}
    return s;
  }}
}}
const an = new Anim(); const m = new Map<string, V>(); m.set('x', v); an.tr.set(k, m);
console.log(an.apply());
"
    ));
    let (kf, vf, size) = (recv(&pf, "kf"), recv(&pf, "vf"), recv(&pf, "size"));
    eprintln!("DESTR nested kf={kf:?} vf={vf:?} size={size:?}");
    assert!(
        !has(&kf, "Map") && !has(&size, "K") && !has(&vf, "K") && !has(&vf, "Map"),
        "positions merged: kf={kf:?} vf={vf:?} size={size:?}"
    );
}

// DESTR-POS RED: the elements of an array literal bound in place.
#[test]
#[ignore = "DESTR-LIT open: an array literal is not index-exact (no index slots), so its constant-index reads merge"]
fn destr_pos_literal_source() {
    let pf = facts(&format!(
        "{KV}function f(x: any) {{ const [p, q] = x; console.log(q.vf, p.kf); }}
f([k, v]);
"
    ));
    let (kf, vf) = (recv(&pf, "kf"), recv(&pf, "vf"));
    eprintln!("DESTR param kf={kf:?} vf={vf:?}");
    assert!(!has(&kf, "V") && !has(&vf, "K"), "positions merged: kf={kf:?} vf={vf:?}");
}

// DESTR-POS RED: a nested pattern reads its own positions at each level.
#[test]
#[ignore = "DESTR-LIT open: an array literal is not index-exact (no index slots), so its constant-index reads merge"]
fn destr_pos_nested_pattern() {
    let pf = facts(&format!(
        "{KV}const t: [[K, V], W] = [[k, v], w];
const [[p, q], r] = t;
console.log(q.vf, p.kf, r.wf);
"
    ));
    let (kf, vf, wf) = (recv(&pf, "kf"), recv(&pf, "vf"), recv(&pf, "wf"));
    eprintln!("DESTR nested-pattern kf={kf:?} vf={vf:?} wf={wf:?}");
    assert!(
        !has(&kf, "V") && !has(&kf, "W") && !has(&vf, "K") && !has(&vf, "W") && !has(&wf, "K"),
        "positions merged: kf={kf:?} vf={vf:?} wf={wf:?}"
    );
}

// Control, hole: `[, q]` of a rest array (index-exact) reads position 1.
#[test]
fn destr_pos_hole() {
    let pf = facts(&format!(
        "{KV}function f(...xs: any[]) {{ const [, q] = xs; console.log(q.vf); }}
f(k, v);
"
    ));
    let vf = recv(&pf, "vf");
    eprintln!("DESTR hole vf={vf:?}");
    assert!(has(&vf, "V") && !has(&vf, "K"), "hole: vf={vf:?}");
}

// DESTR-POS RED: a default puts the pattern in its close `try` (RP1 missed
// it): p is element 0 or the default, q element 1 of the rest array.
#[test]
fn destr_pos_default() {
    let pf = facts(&format!(
        "{KV}function f(...xs: any[]) {{ const [p = w, q] = xs; console.log(q.vf, p.kf); }}
f(k, v);
"
    ));
    let (kf, vf) = (recv(&pf, "kf"), recv(&pf, "vf"));
    eprintln!("DESTR default kf={kf:?} vf={vf:?}");
    assert!(has(&kf, "K") && has(&vf, "V") && !has(&vf, "K") && !has(&vf, "W"), "default: kf={kf:?} vf={vf:?}");
}

// Control, rest: p is position 0; the rest element drains positions >= 1
// with an any-index read (it holds V and W).
#[test]
fn destr_pos_rest() {
    let pf = facts(&format!(
        "{KV}function f(...xs: any[]) {{ const [p, ...r] = xs; console.log(p.kf, r[0].vf, r[1].wf); }}
f(k, v, w);
"
    ));
    let (kf, vf, wf) = (recv(&pf, "kf"), recv(&pf, "vf"), recv(&pf, "wf"));
    eprintln!("DESTR rest kf={kf:?} vf={vf:?} wf={wf:?}");
    assert!(!has(&kf, "V") && !has(&kf, "W") && has(&vf, "V") && has(&wf, "W"), "rest: kf={kf:?} vf={vf:?} wf={wf:?}");
}

// Control, non-array iterable: a generator's pattern runs the iterator
// protocol (unchanged): both positions see the same values.
#[test]
fn destr_pos_generator_unchanged() {
    let pf = facts(&format!(
        "{KV}function* g(): any {{ yield k; yield v; }}
const [p, q] = g();
console.log(q.vf, p.kf);
"
    ));
    let (kf, vf) = (recv(&pf, "kf"), recv(&pf, "vf"));
    eprintln!("DESTR generator kf={kf:?} vf={vf:?}");
    assert_eq!(kf, vf, "generator: the protocol arm reads any position");
}
