//! A failed `x instanceof ArrayBuffer` (`SharedArrayBuffer`): the code it
//! selects reads `x` without the built-in buffer objects of that kind. The
//! compiled program answers the bare built-in name by the cell's buffer
//! brand (perry-runtime instanceof/static_dispatch.rs), so a built-in
//! ArrayBuffer never reaches the false side. Only the intrinsic binding
//! narrows: a program class, a shadowing local or a written global leaves
//! the set as it is; a typed array, ⊤ and every other value pass.
use super::*;

fn mono_m(pf: &ProgramFacts) -> bool {
    let v: Vec<_> = (pf.sites.iter())
        .filter(|s| s.key == "m" && s.is_call)
        .map(|s| s.verdict.clone())
        .collect();
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::T0Mono))
}

fn parse_with(head: &str, test: &str, args: &str) -> String {
    format!(
        "class T {{ m() {{ return 1; }} }}
{head}
function parse(d: any): any {{
  let j: any;
  if (typeof d === \"string\") {{ j = new T(); }}
  else if ({test}) {{ j = new T(); }}
  else {{ j = d; }}
  return j;
}}
{args}
console.log(parse(new ArrayBuffer(4)).m());
console.log(parse(new T()).m());
"
    )
}

/// The GLTFLoader.parse shape: the final `else` never holds the buffer.
#[test]
fn failed_instanceof_array_buffer_drops_the_buffer() {
    let pf = facts(&parse_with("", "d instanceof ArrayBuffer", ""));
    assert!(mono_m(&pf), "{}", pf.dump_text());
}

/// The same through `!` and an early exit.
#[test]
fn negated_and_early_exit_drop_the_buffer() {
    let pf = facts(
        "class T { m() { return 1; } }
function f(d: any): any { if (!(d instanceof ArrayBuffer)) { return d; } return new T(); }
function g(d: any): any { if (d instanceof ArrayBuffer) return new T(); return d; }
console.log(f(new ArrayBuffer(4)).m(), f(new T()).m());
console.log(g(new ArrayBuffer(4)).m(), g(new T()).m());
",
    );
    assert!(mono_m(&pf), "{}", pf.dump_text());
}

/// `SharedArrayBuffer` drops its own kind only.
#[test]
fn shared_kind_is_its_own() {
    let own = facts(
        "class T { m() { return 1; } }
function f(d: any): any { if (d instanceof SharedArrayBuffer) return new T(); return d; }
console.log(f(new SharedArrayBuffer(4)).m(), f(new T()).m());
",
    );
    assert!(mono_m(&own), "{}", own.dump_text());
    let other = facts(
        "class T { m() { return 1; } }
function f(d: any): any { if (d instanceof SharedArrayBuffer) return new T(); return d; }
console.log(f(new ArrayBuffer(4)).m(), f(new T()).m());
",
    );
    assert!(!mono_m(&other), "{}", other.dump_text());
}

/// Controls: every one keeps a value that is no `T` in the `else`.
#[test]
fn controls_keep_the_set() {
    // A typed array is no ArrayBuffer.
    let pf = facts(&parse_with("", "d instanceof ArrayBuffer", "parse(new Uint8Array(4)).m();"));
    assert!(!mono_m(&pf), "typed array {}", pf.dump_text());
    // Unknown code: ⊤ passes.
    let pf = facts(&parse_with(
        "",
        "d instanceof ArrayBuffer",
        "parse(new Function(process.argv[2])()).m();",
    ));
    assert!(!mono_m(&pf), "top {}", pf.dump_text());
    // A program class on the right: no narrowing.
    let pf = facts(&parse_with("class Other {}", "d instanceof Other", ""));
    assert!(!mono_m(&pf), "program class {}", pf.dump_text());
    // A written global binding: no narrowing.
    let pf = facts(&parse_with(
        "(globalThis as any).ArrayBuffer = class Fake {};",
        "d instanceof ArrayBuffer",
        "",
    ));
    assert!(!mono_m(&pf), "written global {}", pf.dump_text());
    // A local that shadows the name: no narrowing.
    let pf = facts(
        "class T { m() { return 1; } }
function f(d: any): any { const ArrayBuffer: any = class Fake {}; if (d instanceof ArrayBuffer) return new T(); return d; }
console.log(f(new (globalThis as any).ArrayBuffer(4)).m(), f(new T()).m());
",
    );
    assert!(!mono_m(&pf), "shadowed {}", pf.dump_text());
}

/// `class X extends ArrayBuffer`: the compiled program allocates an
/// ordinary object with the subclass chain and no buffer brand, so the
/// runtime's brand test answers false and the instance takes the false
/// side; it is kept (here the solver holds it as ⊤).
#[test]
fn subclass_instance_is_kept() {
    let pf = facts(
        "class T { m() { return 1; } }
class X extends ArrayBuffer {}
function f(d: any): any { if (d instanceof ArrayBuffer) return new T(); return d; }
console.log(f(new X(4)).m(), f(new T()).m());
",
    );
    assert!(!mono_m(&pf), "{}", pf.dump_text());
}
