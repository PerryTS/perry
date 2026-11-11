//! Held lengths (`held_lengths`): a local whose Arrays keep their length
//! while its function runs.

use super::*;
use crate::FactsConfig;

/// `(stable_length, stable_front)` of the parameter or top-level `let`
/// named `local` of function or method `func`.
fn fact(src: &str, func: &str, local: &str, cfg: FactsConfig) -> (bool, bool) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let f = hir
        .functions
        .iter()
        .chain(
            hir.classes
                .iter()
                .flat_map(|c| c.methods.iter().chain(c.static_methods.iter())),
        )
        .find(|f| f.name == func)
        .unwrap_or_else(|| panic!("function {func}"));
    let id = f
        .params
        .iter()
        .find(|p| p.name == local)
        .map(|p| p.id)
        .or_else(|| {
            f.body.iter().find_map(|s| match s {
                perry_hir::Stmt::Let { id, name, .. } if name == local => Some(*id),
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("local {local} of {func}"));
    let pf = facts_with(src, cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let b = &mb
        .locals
        .iter()
        .find(|(l, _)| *l == id)
        .unwrap_or_else(|| panic!("local {id} of {func}"))
        .1;
    (b.stable_length, b.stable_front)
}

fn held(src: &str, func: &str, local: &str) -> bool {
    fact(src, func, local, FactsConfig::default()).0
}

const SRC: &str = r#"
function gen(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
// The negative cases bear their Arrays here: a token is its birth site, so
// a field or a closure that held `gen`'s Array would end the fact for the
// positive cases too.
function mk(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function touch(v: number): number { return v + 1; }
function grow(a: number[]): void { a.push(1); }

function walk(n: number): number {
  const xs = gen(n);
  let s = 0;
  for (let i = 0; i < xs.length; i++) s += touch(xs[i] ?? 0);
  return s;
}
function rec(d: number): number {
  const xs = gen(3);
  let s = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] ?? 0;
    if (d > 0) s += rec(d - 1);
  }
  return s;
}
function copied(n: number): number {
  const xs = gen(n);
  const ys = xs.slice();
  let s = 0;
  for (let i = 0; i < ys.length; i++) s += touch(ys[i] ?? 0);
  return s;
}
function handed(): number {
  const xs = mk(3);
  let s = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] ?? 0;
    if (xs.length < 6) grow(xs);
  }
  return s;
}
function closed(): number {
  const xs = mk(3);
  const more = () => { if (xs.length < 6) xs.push(7); };
  let s = 0;
  for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; more(); }
  return s;
}
class Holder {
  items: number[] = mk(3);
  more(): void { if (this.items.length < 6) this.items.push(9); }
  walk(): number {
    const xs = this.items;
    let s = 0;
    for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; this.more(); }
    return s;
  }
}
console.log(walk(4), rec(2), copied(3), handed(), closed(), new Holder().walk());
"#;

#[test]
fn a_fresh_result_walked_across_calls_has_a_stable_length() {
    // `xs` holds what `gen` returned: no field holds it, `walk` hands it to
    // no call and has no length event on it. Its birth is a literal, so
    // its length is defined.
    assert_eq!(
        fact(SRC, "walk", "xs", FactsConfig::default()),
        (true, true)
    );
}

#[test]
fn the_birth_function_running_again_below_the_loop_is_no_event() {
    // `rec` calls itself, and `gen` pushes onto another Array of the same
    // birth site: tokens are per site, the fact is per activation.
    assert!(held(SRC, "rec", "xs"));
}

#[test]
fn a_slice_has_a_stable_length_and_no_defined_one() {
    // A `slice` birth records no length (`solve::arr_len`).
    assert_eq!(
        fact(SRC, "copied", "ys", FactsConfig::default()),
        (true, false)
    );
}

#[test]
fn the_function_that_pushes_has_no_stable_length() {
    // `gen` pushes onto `out`; `grow` pushes onto its parameter.
    assert!(!held(SRC, "gen", "out"));
    assert!(!held(SRC, "grow", "a"));
}

#[test]
fn an_array_handed_to_a_call_has_no_stable_length() {
    assert!(!held(SRC, "handed", "xs"));
}

#[test]
fn an_array_a_closure_captures_has_no_stable_length() {
    assert!(!held(SRC, "closed", "xs"));
}

#[test]
fn an_array_a_field_holds_has_no_stable_length() {
    let module = perry_parser::parse_typescript(SRC, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let walk = hir
        .classes
        .iter()
        .flat_map(|c| c.methods.iter())
        .find(|f| f.name == "walk")
        .expect("Holder.walk");
    let id = walk
        .body
        .iter()
        .find_map(|s| match s {
            perry_hir::Stmt::Let { id, name, .. } if name == "xs" => Some(*id),
            _ => None,
        })
        .expect("xs");
    let pf = facts(SRC);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let b = &mb.locals.iter().find(|(l, _)| *l == id).expect("local").1;
    assert!(!b.stable_length);
}
