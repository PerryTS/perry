//! Named lengths (`named_lengths`): a local through whose name alone its
//! functions change the length of its Arrays.

use super::*;
use crate::FactsConfig;

/// `named_length` of the parameter or top-level `let` named `local` of
/// function or method `func`.
fn fact(src: &str, func: &str, local: &str, cfg: FactsConfig) -> bool {
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
    mb.locals
        .iter()
        .find(|(l, _)| *l == id)
        .unwrap_or_else(|| panic!("local {id} of {func}"))
        .1
        .named_length
}

fn named(func: &str, local: &str) -> bool {
    fact(SRC, func, local, FactsConfig::default())
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
  const xs: number[] | undefined = n > 0 ? [1, 2, 3] : undefined;
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
function elsewhere(a: number[]): void { a.push(1); }
function reader(): number {
  const xs = [1, 2, 3];
  elsewhere(xs);
  const out = [0, 0, 0, 0];
  let s = 0;
  for (let i = 0; i < xs.length; i++) { out[i] = xs[i] ?? 0; s += out.length; }
  return s;
}
const same = [1, 2, 3, 4];
console.log(own(3), guarded(1), picked(), pair(same, same), apart([1, 2], [3, 4]),
  new Bag().walk(), reader());
"#;

#[test]
fn a_local_stored_through_its_own_name_has_a_named_length() {
    // `xs.push` and `xs[i] = …` both have `xs` as their receiver.
    assert!(named("own", "xs"));
}

#[test]
fn a_narrowed_read_is_a_read_of_the_name() {
    // Inside `if (xs)` a read of `xs` sees the guard's node.
    assert!(named("guarded", "xs"));
}

#[test]
fn an_array_popped_through_another_name_has_no_named_length() {
    // `xs` may hold the Array of `POPS`, which `picked` pops by that name.
    assert!(!named("picked", "xs"));
}

#[test]
fn two_parameters_that_may_be_one_array_have_no_named_length() {
    assert!(!named("pair", "a"));
    // `b`'s own events all go through `b`.
    assert!(named("pair", "b"));
}

#[test]
fn two_parameters_that_are_never_one_array_keep_it() {
    assert!(named("apart", "a"));
}

#[test]
fn a_store_through_the_field_that_holds_the_array_ends_it() {
    assert!(!named("walk", "xs"));
}

#[test]
fn an_event_in_a_function_that_cannot_name_the_local_is_none() {
    // `elsewhere` pushes onto the Array, but while a loop of `reader`
    // without calls runs, `elsewhere` does not; `out` is another Array.
    assert!(named("reader", "xs"));
}

#[test]
fn the_negative_control_claims_it_for_every_array_local() {
    let cfg = FactsConfig {
        sabotage: Some("namedlen".to_string()),
        ..FactsConfig::default()
    };
    assert!(fact(SRC, "picked", "xs", cfg));
}
