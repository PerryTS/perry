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

#[test]
fn t1_parameters_carry_their_interval() {
    let s = sig(ENCODE, "encode").expect("a signature");
    assert_eq!(s.ranges[0], Some((0, 127)), "{s:?}");
    assert!(
        s.ranges.iter().all(|r| r.is_some_and(|(lo, _)| lo >= 0)),
        "{s:?}"
    );
}

#[test]
fn t4a_an_overflowing_sum_returns_f64() {
    let src = r#"
function add(a: number, b: number): number { return a + b; }
function addw(a: number, b: number): number { return (a + b) | 0; }
console.log(add(2147483647, 1), addw(2147483647, 1));
"#;
    let s = sig(src, "add").expect("a signature");
    assert_eq!(s.ret, F64, "{s:?}");
}

const AT: &str = r#"
class B {
  board: number[] = new Array<number>(128).fill(0);
  at(i: number): number { return this.board[i] ?? 0; }
}
const b = new B();
let s = 0;
for (let i = 0; i < 128; i++) s += b.at(i);
"#;

#[test]
fn t4b_an_in_range_index_parameter_keeps_its_interval() {
    let src = format!("{AT}console.log(s);\n");
    let s = sig(&src, "at").expect("a signature");
    assert_eq!(s.params[0], I32, "{s:?}");
    assert_eq!(s.ranges[0], Some((0, 127)), "{s:?}");
}

#[test]
fn t4b_a_fractional_index_makes_the_parameter_f64() {
    let src = format!("{AT}console.log(s, b.at(200), b.at(1.5));\n");
    let s = sig(&src, "at").expect("a signature");
    assert_eq!(s.params[0], F64, "{s:?}");
    assert_eq!(s.ranges[0], None, "{s:?}");
}

#[test]
fn t4c_layout_breakers_drop_the_layout() {
    for extra in [
        "const q = new P(3, 4); delete (q as any).y; console.log(q.sum(1));",
        "console.log((Object.setPrototypeOf({ x: 10, z: 1, y: 20 }, P.prototype) as any).sum(3));",
        "const o = Object.create(P.prototype); o.x = 1; o.y = 2; console.log(o.sum(3));",
    ] {
        let src = format!("{SUM}\n{extra}\n");
        let s = sig(&src, "sum");
        // Codegen takes the layout only with `this_class` AND the class's
        // `fixed_layout` (§3.2): either one false refuses it.
        let pf = facts(&src);
        let fixed = s.as_ref().and_then(|s| s.this_class).is_some_and(|c| {
            pf.templates
                .iter()
                .any(|t| t.class_id == Some(c) && t.fixed_layout)
        });
        assert!(!fixed, "{extra}: {s:?}");
    }
}

#[test]
fn s21_a_fractional_caller_makes_a_tier_a_slot_f64() {
    let src = r#"
function step(x: number): number { return x * 2; }
let s = 0;
for (let i = 0; i < 100; i++) s += step(i);
console.log(s, step(0.5));
"#;
    let s = sig(src, "step").expect("a signature");
    assert_eq!(s.params, vec![F64], "{s:?}");
}

#[test]
fn s22_a_string_through_apply_makes_the_helper_boxed() {
    let src = r#"
function inc(x: number): number { return (x + 1) | 0; }
let s = 0;
for (let i = 0; i < 10; i++) s += inc(i);
console.log(s, (inc as any).apply(null, ["7"]));
"#;
    let s = sig(src, "inc");
    assert!(s.as_ref().is_none_or(|s| s.params == vec![Boxed]), "{s:?}");
}

#[test]
fn s24_an_overriding_subclass_drops_the_layout() {
    let src = r#"
class P {
  x = 1;
  base(): number { return 1; }
  sum(k: number): number { return (this.x + this.base() + k) | 0; }
}
class Q extends P { base(): number { return 100; } }
const p = new P();
let s = 0;
for (let i = 0; i < 10; i++) s += p.sum(i);
console.log(s, new Q().sum(1));
"#;
    let s = sig(src, "sum");
    assert!(s.as_ref().is_none_or(|s| s.this_class.is_none()), "{s:?}");
}

/// The `(offset, end)` of every direct call of `name` in `m`'s top level.
fn calls_of(m: &mut perry_hir::Module, name: &str, f: &mut dyn FnMut(&mut u32, &mut u32)) {
    use perry_hir::{Expr, Stmt};
    let id = m
        .functions
        .iter()
        .find(|x| x.name == name)
        .expect("a function")
        .id;
    fn walk(e: &mut Expr, id: u32, f: &mut dyn FnMut(&mut u32, &mut u32)) {
        if let Expr::Call {
            callee,
            byte_offset,
            byte_end,
            ..
        } = e
        {
            if matches!(callee.as_ref(), Expr::FuncRef(x) if *x == id) {
                f(byte_offset, byte_end);
            }
        }
        perry_hir::walker::walk_expr_children_mut(e, &mut |c| walk(c, id, f));
    }
    for s in &mut m.init {
        match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) => walk(e, id, f),
            _ => {}
        }
    }
}

#[test]
fn s20_distinct_calls_under_one_key_refuse() {
    // E3 (CWN 11-09): the cross-module inliner keeps a call's offset, so two
    // distinct calls may share a key. Here the second `g` call takes the
    // first's offset (its own span end kept): the key names two source
    // calls, and the site refuses, although both resolve to `g` and pass
    // int32 arguments.
    let src = r#"
function g(x: number): number { return (x * 3) | 0; }
const a = g(1);
const b = g(20);
console.log(a + b);
"#;
    let mut m = hir(src);
    let mut spans: Vec<(u32, u32)> = Vec::new();
    calls_of(&mut m, "g", &mut |o, e| spans.push((*o, *e)));
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert_ne!(spans[0].1, spans[1].1);
    let first = spans[0].0;
    let mut k = 0;
    calls_of(&mut m, "g", &mut |o, _| {
        if k == 1 {
            *o = first;
        }
        k += 1;
    });
    let pf = compute(&[("t.ts".to_string(), &m)], &FactsConfig::default());
    let typed: Vec<u32> = pf
        .bindings
        .iter()
        .flat_map(|b| b.typed_calls.iter().map(|(o, _)| *o))
        .collect();
    assert!(!typed.contains(&first), "{typed:?}");
    // Unchanged, both sites are typed.
    let id = id_of(src, "g");
    assert_eq!(typed_targets(src), vec![id, id]);
}

#[test]
fn e3_copies_of_one_call_join() {
    // A for…of body is walked in its counted and its IteratorRecord arm, so
    // gen sees two HIR calls of `mf` under one key, with one span: they are
    // joined. (The inliner's copies, `twice` into its callers, arise in the
    // compile pipeline only; the P2 controls check those.)
    let src = r#"
function mf(m: number): number { return (m >> 7) & 127; }
function twice(m: number): number { return mf(m) + 1; }
let s = 0;
for (let i = 0; i < 100; i++) s += twice(i * 129);
function other(k: number): number { return twice(k) * 2; }
for (let i = 0; i < 10; i++) s += other(i);
const xs = [1, 2, 3];
for (const x of xs) s += mf(x);
console.log(s);
"#;
    let id = id_of(src, "mf");
    let t = typed_targets(src);
    assert!(t.iter().filter(|&&f| f == id).count() >= 2, "{t:?}");
}

#[test]
fn e3_a_patched_iterator_arm_with_strings_is_not_typed() {
    // Control (a): the IteratorRecord arm of the for…of passes what a user
    // `Symbol.iterator` yields (a fraction and a String).
    let src = r#"
function inc(x: number): number { return x + 1; }
const xs: number[] = [1, 2, 3];
(xs as any)[Symbol.iterator] = function* () { yield 0.5; yield "x"; };
let s = "";
for (const x of xs) s += inc(x);
for (let i = 0; i < 4; i++) s += inc(i);
console.log(s);
"#;
    let id = id_of(src, "inc");
    assert!(!typed_targets(src).contains(&id), "{:?}", sig(src, "inc"));
}

/// The `# typed_call` probe lines of `src` (the dump's, diagnostics on).
fn typed_call_lines(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        crate::FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# typed_call "))
        .cloned()
        .collect()
}

/// A board helper whose key and value are int32s of either sign: both
/// parameters are I32 with intervals that start below 0, and every call
/// site is typed. (Whether codegen keeps the method's index clones over
/// this signature is its selector's `signed_index_params` rule.)
const SIGNED_SET: &str = r#"
class Board {
  cells: number[] = new Array<number>(64).fill(0);
  keys: number[] = [];
  h = 0;
  set(square: number, piece: number): void {
    const old = this.cells[square] ?? 0;
    if (old !== 0) this.h ^= this.keys[(old + 6) * 64 + square] ?? 0;
    this.cells[square] = piece;
    if (piece !== 0) this.h ^= this.keys[(piece + 6) * 64 + square] ?? 0;
  }
}
const b = new Board();
for (let i = 0; i < 64; i++) b.set(i, (i % 13) - 6);
b.set(-8, -3);
console.log(b.h);
"#;

#[test]
fn a_signed_key_and_value_are_i32_parameters_with_their_intervals() {
    let s = sig(SIGNED_SET, "set").expect("a signature");
    assert_eq!(s.params, vec![I32, I32], "{s:?}");
    assert!(s.ranges[0].is_some_and(|(lo, hi)| lo <= -8 && hi >= 63), "{s:?}");
    assert!(s.ranges[1].is_some_and(|(lo, hi)| lo <= -6 && hi >= 6), "{s:?}");
    let id = id_of(SIGNED_SET, "set");
    assert_eq!(typed_targets(SIGNED_SET), vec![id, id]);
}

#[test]
fn the_probe_names_each_typed_call_and_each_refusal() {
    let lines = typed_call_lines(SIGNED_SET);
    let typed = lines
        .iter()
        .filter(|l| l.contains(" callee=set typed=1"))
        .count();
    assert_eq!(typed, 2, "{lines:#?}");
    // A fraction makes `piece` a double: the calls are typed still (F64
    // slot). A string argument is no Number: that call is refused, by
    // argument and reason.
    let src = format!("{SIGNED_SET}\nfunction pick(i: number): number {{ return (i * 2) | 0; }}\nlet t = 0;\nfor (let i = 0; i < 9; i++) t += pick(i);\nconsole.log(t);\n");
    let lines = typed_call_lines(&src);
    assert!(
        lines.iter().any(|l| l.contains(" callee=pick typed=1")),
        "{lines:#?}"
    );
    // An override: two bodies, no typed call, the probe names both.
    let src = r#"
class A { f(x: number): number { return x | 0; } }
class B extends A { f(x: number): number { return (x * 2) | 0; } }
const xs: A[] = [new A(), new B()];
let s = 0;
for (let i = 0; i < 4; i++) s += xs[i & 1].f(i);
console.log(s);
"#;
    let lines = typed_call_lines(src);
    assert!(
        lines
            .iter()
            .any(|l| l.contains(" callee=f typed=0 why=method-not-one-target")),
        "{lines:#?}"
    );
    assert!(!lines.iter().any(|l| l.contains("typed=1")), "{lines:#?}");
}

#[test]
fn without_diagnostics_there_is_no_probe_line() {
    let pf = facts(SIGNED_SET);
    assert!(
        !pf.rep_probe.iter().any(|l| l.starts_with("# typed_call ")),
        "{:#?}",
        pf.rep_probe
    );
}
