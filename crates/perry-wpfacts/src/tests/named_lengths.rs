//! Named lengths (`named_lengths`): a local through whose name alone its
//! functions change the length of its Arrays.
//!
//! The fact is asked only of `Rep::Arr` locals, so every case asserts the
//! representation first (`Case::arr`): a local that is no `Arr` in this
//! solve would have no named length whatever the rule says. A negative
//! case asserts the reason of its refusal (the `# named_len … refused=`
//! line of the dump) beside a positive twin that differs only in the
//! refused construct.

use super::*;
use crate::bindings::Rep;
use crate::FactsConfig;

/// What the solve says of one local.
struct Case {
    /// The local is a plain program Array (`Rep::Arr`): the fact's
    /// precondition.
    arr: bool,
    named_length: bool,
    /// The local's `# named_len` dump line (none: the rule never judged it).
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
    let head = format!("# named_len module=0 local={id} ");
    Case {
        arr: matches!(b.rep, Rep::Arr(_)),
        named_length: b.named_length,
        line: pf
            .dump_text()
            .lines()
            .find(|l| l.starts_with(&head))
            .map(str::to_string),
    }
}

/// `func`'s `local` is an `Arr` with a named length.
#[track_caller]
fn assert_named(func: &str, local: &str) {
    let c = case_with(SRC, func, local, FactsConfig::default());
    assert!(c.arr, "{func}.{local} must be an Arr in this solve");
    assert!(
        c.named_length,
        "{func}.{local} has no named length: {:?}",
        c.line
    );
}

/// `func`'s `local` is an `Arr`, and the rule refuses its named length for
/// `reason` (the head of the dump line's reason).
#[track_caller]
fn assert_refused(func: &str, local: &str, reason: &str) {
    let c = case_with(SRC, func, local, FactsConfig::default());
    assert!(
        c.arr,
        "{func}.{local} must be an Arr in this solve, or the case asserts nothing about the rule"
    );
    assert!(!c.named_length, "{func}.{local} has a named length");
    let line = c
        .line
        .unwrap_or_else(|| panic!("no named_len line for {func}.{local}"));
    let why = line
        .split_once(" refused=")
        .unwrap_or_else(|| panic!("{func}.{local} is not refused: {line}"))
        .1;
    assert!(
        why.starts_with(reason),
        "{func}.{local} is refused for another reason than {reason}: {why}"
    );
}

// Each case bears its Arrays at its own site: a token is its birth site.
const SRC: &str = r#"
let k = 1;
function own(n: number): number {
  const xs: number[] = [];
  for (let i = 0; i < n; i++) xs.push(i);
  for (let i = 0; i < xs.length; i++) xs[i] = i + 1;
  return xs.length;
}
function guarded(n: number): number {
  const xs = [n, 2, 3];
  let s = 0;
  if (xs) { for (let i = 0; i < xs.length; i++) { xs[i] = i; s += xs.length; } }
  return s;
}
const POPS: number[] = [1, 2, 3, 4, 5, 6];
const KEPT: number[] = [7, 8, 9];
function picked(): number {
  const xs = k ? POPS : KEPT;
  let s = 0;
  for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; POPS.pop(); }
  return s;
}
const POPS2: number[] = [1, 2, 3, 4, 5, 6];
const KEPT2: number[] = [7, 8, 9];
function pickedTwin(): number {
  const xs = k ? POPS2 : KEPT2;
  let s = 0;
  for (let i = 0; i < xs.length; i++) { s += xs[i] ?? 0; xs.pop(); }
  return s;
}
function pair(a: number[], b: number[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) { s += a[i] ?? 0; b.pop(); }
  return s;
}
function apart(a: number[], b: number[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) { s += a[i] ?? 0; b.pop(); }
  return s;
}
class Bag {
  items: number[] = [1, 2, 3];
  walk(): number {
    const xs = this.items;
    let s = 0;
    for (let i = 0; i < xs.length; i++) {
      s += xs[i] ?? 0;
      if (this.items.length < 6) this.items[this.items.length] = 0;
    }
    return s;
  }
}
class BagTwin {
  items: number[] = [1, 2, 3];
  walkOwn(): number {
    const xs = this.items;
    let s = 0;
    for (let i = 0; i < xs.length; i++) {
      s += xs[i] ?? 0;
      if (xs.length < 6) xs[xs.length] = 0;
    }
    return s;
  }
}
function elsewhere(a: number[]): void { a.push(1); }
function reader(): number {
  const xs = [1, 2, 3];
  elsewhere(xs);
  const out = [0, 0, 0, 0];
  let s = 0;
  for (let i = 0; i < xs.length; i++) { out[i] = xs[i] ?? 0; s += out.length; }
  return s;
}
function popper(a: number[], out: number[]): number {
  for (let i = 0; i < a.length; i++) { out.pop(); out.push(a[i] ?? 0); }
  return out.length;
}
const same = [1, 2, 3, 4];
console.log(own(3), guarded(1), picked(), pickedTwin(), pair(same, same), apart([1, 2], [3, 4]),
  new Bag().walk(), new BagTwin().walkOwn(), reader(), popper([1, 2, 3], [9]), popper([4], [5, 6]));
"#;

#[test]
fn a_local_stored_through_its_own_name_has_a_named_length() {
    // `xs.push` and `xs[i] = …` both have `xs` as their receiver.
    assert_named("own", "xs");
}

#[test]
fn a_narrowed_read_is_a_read_of_the_name() {
    // Inside `if (xs)` a read of `xs` sees the guard's node.
    assert_named("guarded", "xs");
}

#[test]
fn an_array_popped_through_another_name_has_no_named_length() {
    // `xs` may hold the Array of `POPS`, which `picked` pops by that name;
    // the twin pops `xs`.
    assert_refused("picked", "xs", "event:other-name(");
    assert_named("pickedTwin", "xs");
}

#[test]
fn two_parameters_that_may_be_one_array_have_no_named_length() {
    assert_refused("pair", "a", "event:other-name(");
    // `b`'s own events all go through `b`.
    assert_named("pair", "b");
}

#[test]
fn two_parameters_that_are_never_one_array_keep_it() {
    // The twin of `pair`: the same body, called with two Arrays.
    assert_named("apart", "a");
}

#[test]
fn a_store_through_the_field_that_holds_the_array_ends_it() {
    // The twin stores through `xs`.
    assert_refused("walk", "xs", "event:other-name(");
    assert_named("walkOwn", "xs");
}

#[test]
fn an_event_in_a_function_that_cannot_name_the_local_is_none() {
    // `elsewhere` pushes onto the Array, but while a loop of `reader`
    // without calls runs, `elsewhere` does not; `out` is another Array.
    assert_named("reader", "xs");
}

#[test]
fn a_pop_through_a_parameter_is_placed_in_its_function() {
    // `out.pop()` has no store: its rule is placed by its receiver's site,
    // in a summary instance too.
    assert_named("popper", "out");
    assert_named("popper", "a");
}

#[test]
fn the_negative_control_claims_it_for_every_array_local() {
    let cfg = FactsConfig {
        sabotage: Some("namedlen".to_string()),
        ..FactsConfig::default()
    };
    let c = case_with(SRC, "picked", "xs", cfg);
    assert!(c.arr && c.named_length, "{:?}", c.line);
}
