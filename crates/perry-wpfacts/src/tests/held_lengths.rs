//! Held lengths (`held_lengths`): a local whose Arrays keep their length
//! while its function runs.
//!
//! The fact is asked only of `Rep::Arr` locals, so a local that is no `Arr`
//! in this solve has no stable length whatever the rules say. Every case
//! therefore asserts the representation first (`Case::arr`), and a negative
//! case asserts the rule that refused it (the `# held_len … refused=` line
//! of the dump) beside a positive twin that differs only in the refused
//! construct.

use super::*;
use crate::bindings::Rep;
use crate::FactsConfig;

/// What the solve says of one local.
struct Case {
    /// The local is a plain program Array (`Rep::Arr`): the fact's
    /// precondition.
    arr: bool,
    stable_length: bool,
    stable_front: bool,
    /// The local's `# held_len` dump line (none: the rule never judged it).
    line: Option<String>,
}

/// The parameter or top-level `let` named `local` of function or method
/// `func`.
fn case_with(src: &str, func: &str, local: &str, cfg: FactsConfig) -> Case {
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
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..cfg
        },
    );
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let b = &mb
        .locals
        .iter()
        .find(|(l, _)| *l == id)
        .unwrap_or_else(|| panic!("local {id} of {func}"))
        .1;
    let head = format!("# held_len module=0 local={id} ");
    Case {
        arr: matches!(b.rep, Rep::Arr(_)),
        stable_length: b.stable_length,
        stable_front: b.stable_front,
        line: pf
            .dump_text()
            .lines()
            .find(|l| l.starts_with(&head))
            .map(str::to_string),
    }
}

fn case(func: &str, local: &str) -> Case {
    case_with(SRC, func, local, FactsConfig::default())
}

/// `func`'s `local` is an `Arr` whose length is held.
#[track_caller]
fn assert_held(func: &str, local: &str, front: bool) {
    let c = case(func, local);
    assert!(c.arr, "{func}.{local} must be an Arr in this solve");
    assert!(
        c.stable_length,
        "{func}.{local} has no stable length: {:?}",
        c.line
    );
    assert_eq!(c.stable_front, front, "{func}.{local} front");
}

/// `func`'s `local` is an `Arr`, and rule `rule` (the head of one of the
/// dump line's reasons) refuses its stable length.
#[track_caller]
fn assert_refused(func: &str, local: &str, rule: &str) {
    let c = case(func, local);
    assert!(
        c.arr,
        "{func}.{local} must be an Arr in this solve, or the case asserts nothing about the rule"
    );
    assert!(!c.stable_length, "{func}.{local} has a stable length");
    assert!(!c.stable_front, "{func}.{local} has a stable front");
    let line = c
        .line
        .unwrap_or_else(|| panic!("no held_len line for {func}.{local}"));
    let why = line
        .split_once(" refused=")
        .unwrap_or_else(|| panic!("{func}.{local} is not refused: {line}"))
        .1;
    assert!(
        why.split('+').any(|r| r.starts_with(rule)),
        "{func}.{local} is refused by another rule than {rule}: {why}"
    );
}

// Each case bears its Arrays at its own site (`genW`, `genH`, …): a token is
// its birth site, so a field or a closure that held another case's Array
// would end the fact for that case too. A negative case and its positive
// twin (`…Twin`) differ only in the refused construct.
const SRC: &str = r#"
let k = 0;
function genW(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genR(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genC(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genH(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genHT(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genL(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genLT(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genF(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function genFT(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 2);
  return out;
}
function touch(v: number): number { return v + 1; }
function grow(a: number[]): void { a.push(1); }
function size(a: number[]): number { return a.length; }

function walk(n: number): number {
  const xs = genW(n);
  let s = 0;
  for (let i = 0; i < xs.length; i++) s += touch(xs[i] ?? 0);
  return s;
}
function rec(d: number): number {
  const xs = genR(3);
  let s = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] ?? 0;
    if (d > 0) s += rec(d - 1);
  }
  return s;
}
function copied(n: number): number {
  const xs = genC(n);
  const ys = xs.slice();
  let s = 0;
  for (let i = 0; i < ys.length; i++) s += touch(ys[i] ?? 0);
  return s;
}
function handed(): number {
  const xs = genH(3);
  let s = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] ?? 0;
    if (xs.length < 6) grow(xs);
  }
  return s;
}
function handedTwin(): number {
  const xs = genHT(3);
  let s = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] ?? 0;
    if (xs.length < 6) touch(xs.length);
  }
  return s;
}
function closed(): number {
  const xs = genL(3);
  const more = () => { if (xs.length < 6) xs.push(7); };
  let s = 0;
  for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; more(); }
  return s;
}
function closedTwin(): number {
  const xs = genLT(3);
  const more = () => { if (k < 6) k++; };
  let s = 0;
  for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; more(); }
  return s;
}
class Holder {
  items: number[] = genF(3);
  more(): void { if (this.items.length < 6) this.items.push(9); }
  walkField(): number {
    const xs = this.items;
    let s = 0;
    for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; this.more(); }
    return s;
  }
}
class HolderTwin {
  count: number = 0;
  more(): void { if (this.count < 6) this.count++; }
  walkFrame(): number {
    const xs = genFT(3);
    let s = 0;
    for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; this.more(); }
    return s;
  }
}
console.log(walk(4), rec(2), copied(3), handed(), handedTwin(), closed(), closedTwin(),
  new Holder().walkField(), new HolderTwin().walkFrame(), size(genW(1)));
"#;

#[test]
fn a_fresh_result_walked_across_calls_has_a_stable_length() {
    // `xs` holds what `genW` returned: no field holds it, `walk` hands it to
    // no call and has no length event on it. Its birth is a literal, so
    // its length is defined.
    assert_held("walk", "xs", true);
}

#[test]
fn the_birth_function_running_again_below_the_loop_is_no_event() {
    // `rec` calls itself, and `genR` pushes onto another Array of the same
    // birth site: tokens are per site, the fact is per activation.
    assert_held("rec", "xs", true);
}

#[test]
fn a_slice_has_a_stable_length_and_no_defined_one() {
    // A `slice` birth records no length (`solve::arr_len`).
    assert_held("copied", "ys", false);
}

#[test]
fn the_function_that_pushes_has_no_stable_length() {
    // Rule 3: `genW` pushes onto `out`, `grow` onto its parameter; `size`
    // only reads its parameter's length.
    assert_refused("genW", "out", "event:own:");
    assert_refused("grow", "a", "event:own:");
    assert_held("size", "a", true);
}

#[test]
fn an_array_handed_to_a_call_has_no_stable_length() {
    // Rule 2: `handed` passes `xs` to `grow`; the twin passes `xs.length`
    // to `touch`.
    assert_refused("handed", "xs", "passed:own:");
    assert_held("handedTwin", "xs", true);
}

#[test]
fn an_array_a_closure_captures_has_no_stable_length() {
    // Rule 1: the closure references `xs`; the twin's references `k`.
    assert_refused("closed", "xs", "captured:");
    assert_held("closedTwin", "xs", true);
}

#[test]
fn an_array_a_field_holds_has_no_stable_length() {
    // Rule 1: a field is a heap holder; the twin's Array is in its frame.
    assert_refused("walkField", "xs", "heap:");
    assert_held("walkFrame", "xs", true);
}
