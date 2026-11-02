//! Per-unit mode: the owner of an object learns that another unit called a
//! function value it holds as its own parameter with that object as the
//! receiver. The call travels back as a parameter call whose `this` is the
//! object described (`units::ParamCall::this`, like its arguments,
//! DECISIONS 140); the unit holding the function runs it on a stand-in of
//! the layout, and what it does to the stand-in is its report (`broken`),
//! which the owner's copy takes as any (a broken literal is undescribed
//! and escapes, a broken class is no contract). Where the layout leaves
//! with the function value in a field any reader may name, the marker
//! escapes with the field and the call is one of an unknown function
//! value: the receiver escapes as whole escapes such a receiver. Per unit
//! never claims more than whole.

use super::unit_alias::assert_not_stronger;
use super::*;

fn owner_field<'p>(pf: &'p ProgramFacts, path: &str, key: &str) -> &'p crate::derive::FieldFacts {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with(path))
        .find_map(|t| t.fields.iter().find(|f| f.key == key))
        .unwrap_or_else(|| panic!("no {path} template with field {key}\n{}", pf.dump_text()))
}

fn writable(pf: &ProgramFacts, path: &str, key: &str) {
    let f = owner_field(pf, path, key);
    assert!(
        !f.readonly,
        "{path}.{key} claimed readonly: {f:?}\n{}",
        pf.dump_text()
    );
    assert!(
        f.kind.top || f.kind.prims & crate::model::prim::STR != 0,
        "{path}.{key} kind misses the other unit's store: {f:?}\n{}",
        pf.dump_text()
    );
}

// a births `{ f, n }` with `f` its own parameter and hands it to b, which
// calls `h.f()`; main's function writes `this.n`. The recursion keeps each
// function in its own unit when the modules are compiled.
const A: &str = r#"
import { use } from "./b";
export function make(f, depth) {
  if (depth > 0) return make(f, depth - 1);
  const o = { f, n: 1 };
  use(o, 2);
  return typeof o.n + ":" + (o.n + 1);
}
"#;
const B: &str = r#"
export function use(h, depth) {
  if (depth > 0) return use(h, depth - 1);
  return h.f();
}
"#;
const MAIN: &str = r#"
import { make } from "./a";
console.log(make(function () { this.n = "str"; return 7; }, 2));
"#;

#[test]
fn a_parameter_called_as_a_method_of_an_own_object_by_another_unit_writes_it() {
    // b's `h.f()` comes back to a as a parameter call of `h`'s argument; a
    // replays it on `o`, finds its own parameter `f` under the key and
    // records a parameter call of `make` whose `this` is `o` described;
    // main runs its function on a stand-in of a's layout and stores into
    // it: a's display is reported, a's copy escapes.
    let (u, _w) = assert_not_stronger(&[("a", A), ("b", B), ("main", MAIN)]);
    writable(&u, "a.ts", "n");
    // `o` leaves a as the call's receiver with the caller's function under
    // `f`, a key any reader of the layout may name and call: the field is
    // sunk (`nested_fields`), the marker escapes, and the call is one of an
    // unknown function value, whose receiver escapes as in whole.
    let t = u
        .templates
        .iter()
        .find(|t| t.display.starts_with("a.ts"))
        .unwrap_or_else(|| panic!("no a.ts template\n{}", u.dump_text()));
    assert_eq!(
        t.escape.as_deref(),
        Some("receiver of a call of an unknown function value"),
        "{}",
        u.dump_text()
    );
}

// The receiver described: the function value sits in a private field no
// reader can name, so the layout leaves without it and the marker stays
// the caller's own parameter. The caller runs its function on a stand-in
// of A and stores into it: A is reported, and a's copy of A, broken, is no
// contract and escapes.
const A_PRIV: &str = r#"
export class A {
  #f;
  n = 1;
  constructor(f) { this.#f = f; }
  run() { return this.#f(); }
}
export function make(f) {
  const o = new A(f);
  o.run();
  return typeof o.n + ":" + (o.n + 1);
}
"#;
const MAIN_PRIV: &str = r#"
import { make } from "./a";
console.log(make(function () { this.n = "str"; return 7; }));
"#;

#[test]
fn a_parameter_in_a_private_field_called_as_a_method_runs_on_the_described_receiver() {
    let (u, _w) = assert_not_stronger(&[("a", A_PRIV), ("main", MAIN_PRIV)]);
    writable(&u, "a.ts", "n");
    assert!(
        u.broken.iter().any(|d| d.starts_with("a.ts")),
        "a's display is not broken: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
}

// The broken-layout variant through a closed callee (sys.ts
// `createDynamicPriorityPollingWatchFile({ getModifiedTime, setTimeout })`):
// a's `ensure` births `host` and passes it to the module's closed `poll`,
// which hands it to the exported `readTime`; that use is beyond the
// argument contract, so the census breaks the display and a describes the
// argument as unknown ("broken-layout") from then on. `poll`'s parameter is
// then no longer described (`Gen::closed_params`): its marker keeps its own
// uses in `poll`'s contract, and a applies them to what it passes (an
// escape sinks the argument, `stand_in_fn_of`). main's function, run on
// `host` as `this`, stores a string under `n` through ⊤; only an escaped
// copy is reached by that store, so a's copy must not keep `n` readonly.
const A_CLOSED: &str = r#"
export function readTime(host, f) {
  return host.getTime(f) || 0;
}
function poll(host, depth) {
  if (depth > 0) return poll(host, depth - 1);
  readTime(host, "x");
  return 0;
}
export function createWatch({ getTime }, depth) {
  if (depth > 0) return createWatch({ getTime }, depth - 1);
  function ensure() {
    const host = { getTime, n: 1 };
    poll(host, 2);
    return typeof host.n + ":" + host.n;
  }
  return ensure();
}
"#;
const MAIN_CLOSED: &str = r#"
import { createWatch, readTime } from "./a";
console.log(readTime({ getTime: (f) => 1 }, "y"));
console.log(createWatch({ getTime: function (f) { this.n = "str"; return 7; } }, 2));
"#;

#[test]
fn a_broken_argument_of_a_closed_callee_keeps_the_callee_parameter_uses() {
    let (u, _w) = assert_not_stronger(&[("a", A_CLOSED), ("main", MAIN_CLOSED)]);
    let host = u
        .templates
        .iter()
        .find(|t| t.display.starts_with("a.ts") && t.fields.iter().any(|f| f.key == "n"))
        .unwrap_or_else(|| panic!("no a.ts template with field n\n{}", u.dump_text()));
    assert!(
        u.broken.contains(&host.display),
        "a's display is not broken: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    assert!(
        host.escape.is_some(),
        "a's copy of its broken display did not escape\n{}",
        u.dump_text()
    );
    writable(&u, "a.ts", "n");
}
