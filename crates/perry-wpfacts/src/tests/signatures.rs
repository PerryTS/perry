//! P2: typed signatures (`signatures`, DESIGN-p2 rev 2.1 §7.1). Each test
//! asserts the exact `Signature` the fact computes.
use super::*;
use crate::bindings::Rep::{Boxed, F64, I32};
use crate::Signature;

fn hir(src: &str) -> perry_hir::Module {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    perry_hir::lower_module(&module, "t", "t.ts").expect("lowers")
}

/// The HIR id of the function or method named `name`.
fn id_of(src: &str, name: &str) -> u32 {
    let m = hir(src);
    m.functions
        .iter()
        .chain(m.classes.iter().flat_map(|c| c.methods.iter()))
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function {name}"))
        .id
}

fn sig(src: &str, name: &str) -> Option<Signature> {
    let id = id_of(src, name);
    facts(src)
        .bindings
        .iter()
        .flat_map(|b| b.signatures.iter())
        .find(|(f, _)| *f == id)
        .map(|(_, s)| s.clone())
}

/// The bodies the program's typed call sites serve.
fn typed_targets(src: &str) -> Vec<u32> {
    let mut v: Vec<u32> = facts(src)
        .bindings
        .iter()
        .flat_map(|b| b.typed_calls.iter().map(|(_, (_, f))| *f))
        .collect();
    v.sort_unstable();
    v
}

const ENCODE: &str = r#"
function encode(from: number, to: number, promotion = 0, flags = 0): number {
  return from | (to << 7) | (promotion << 14) | flags;
}
const moves: number[] = [];
for (let f = 0; f < 120; f++) {
  for (let t = 0; t < 120; t += 17) {
    moves.push(encode(f, t));
    moves.push(encode(f, t, 5, 2));
  }
}
let s = 0;
for (let i = 0; i < moves.length; i++) s = (s + (moves[i] & 127)) | 0;
console.log(s);
"#;

#[test]
fn s1_encode_is_typed_and_its_calls_use_it() {
    let s = sig(ENCODE, "encode").expect("a signature");
    assert_eq!(s.params, vec![I32, I32, I32, I32], "{s:?}");
    assert_eq!(s.this_class, None);
    let id = id_of(ENCODE, "encode");
    assert_eq!(typed_targets(ENCODE), vec![id, id]);
}

#[test]
fn s2_a_fraction_makes_that_parameter_f64() {
    let src = r#"
function scale(x: number, k: number): number { return x * k; }
let s = 0;
for (let i = 0; i < 10; i++) s += scale(i, 2) + scale(i, 1.5);
console.log(s);
"#;
    let s = sig(src, "scale").expect("a signature");
    assert_eq!(s.params[1], F64, "{s:?}");
}

#[test]
fn s7_minus_zero_is_f64() {
    let src = r#"
function id(x: number): number { return x * 2; }
console.log(id(1), id(-0));
"#;
    let s = sig(src, "id").expect("a signature");
    assert_eq!(s.params[0], F64, "{s:?}");
}

#[test]
fn s3_a_literal_default_is_the_post_default_value() {
    // REVIEW-p2 H1: the local is {5, 7}; the conversion applies the default.
    let src = r#"
function add(a: number, b = 5): number { return (a + b) | 0; }
function maybe(k: number): number | undefined { return k > 0 ? 7 : undefined; }
let s = 0;
for (let i = 0; i < 3; i++) s += add(i, maybe(i));
console.log(add(1), s);
"#;
    let s = sig(src, "add").expect("a signature");
    assert_ne!(s.params[1], Boxed, "{s:?}");
}

#[test]
fn e1_a_literal_default_after_a_non_literal_one_stays_boxed() {
    let src = r#"
function f(a: number = b, b = 1): number { return a; }
try { console.log(f()); } catch (e) { console.log(e instanceof ReferenceError); }
"#;
    assert!(
        sig(src, "f").is_none_or(|s| s.params == vec![Boxed, Boxed]),
        "{:?}",
        sig(src, "f")
    );
}

#[test]
fn s10_rest_and_arguments_get_no_signature() {
    let src = r#"
function r(...xs: number[]): number { return xs.length; }
function a(x: number): number { return arguments.length + x; }
console.log(r(1, 2), a(3));
"#;
    assert_eq!(sig(src, "r"), None);
    assert_eq!(sig(src, "a"), None);
}

#[test]
fn s14_an_undefined_completion_makes_ret_boxed() {
    let src = r#"
function g(x: number): number | undefined { if (x > 1) return x | 0; }
console.log(g(1), g(3));
"#;
    let s = sig(src, "g");
    assert!(s.as_ref().is_none_or(|s| s.ret == Boxed), "{s:?}");
}

const SUM: &str = r#"
class P {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  sum(k: number): number { return (this.x + this.y + k) | 0; }
}
const p = new P(1, 2);
let s = 0;
for (let i = 0; i < 1000; i++) s += p.sum(i);
console.log(s);
"#;

#[test]
fn e2_one_class_this_is_the_layout() {
    let s = sig(SUM, "sum").expect("a signature");
    assert!(s.this_class.is_some(), "{s:?}");
}

#[test]
fn s19_another_this_drops_the_layout() {
    let src = format!(
        "{SUM}\nconst q = {{ x: 10, z: 100, y: 20 }};\nconsole.log((P.prototype.sum as any).call(q, 3));\n"
    );
    let s = sig(&src, "sum");
    assert!(s.as_ref().is_none_or(|s| s.this_class.is_none()), "{s:?}");
}

#[test]
fn s6_an_override_site_is_not_typed() {
    let src = r#"
class A { f(x: number): number { return x | 0; } }
class B extends A { f(x: number): number { return (x * 2) | 0; } }
const xs: A[] = [new A(), new B()];
let s = 0;
for (let i = 0; i < 4; i++) s += xs[i & 1].f(i);
console.log(s);
"#;
    assert!(typed_targets(src).is_empty());
}

#[test]
fn a_spread_call_is_not_typed() {
    let src = r#"
function h(a: number, b: number): number { return (a + b) | 0; }
const xs = [1, 2];
console.log(h(...xs), h(1, 2));
"#;
    let id = id_of(src, "h");
    assert_eq!(typed_targets(src), vec![id]);
}
