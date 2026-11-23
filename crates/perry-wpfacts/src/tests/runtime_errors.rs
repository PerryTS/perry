//! What a `catch` receives (SEEDS1 THROWN, CATCH2 S2, DESIGN SEEDE section 8):
//! what its try body can throw (its region: the program's throws there, what
//! the functions it calls throw, ⊤ only where unknown code runs in it), the
//! error object a failed runtime operation makes (one realm `Error`,
//! `runtime:Error`), and the shared cell (`THROWN`): rejections, the values a
//! modeled native throws on, and what calls made outside generation throw
//! (protocol, accessor and native callback calls).

use super::*;

fn recv(pf: &ProgramFacts, key: &str) -> (bool, Vec<String>) {
    let s = site(pf, key);
    (s.top, s.receivers.clone())
}

fn has(rs: &[String], name: &str) -> bool {
    rs.iter()
        .any(|r| r.ends_with(&format!(":{name}")) || r == name)
}

/// The fixture: no ⊤ in the program. The caught value of a failed
/// `JSON.parse` is the runtime error; a program object stored on it does not
/// escape, and its own read stays proven.
const FIXTURE: &str = r#"
class P { a = 1; }
const p = new P();
try {
  JSON.parse(process.argv.length > 9 ? "{}" : "{");
} catch (e) {
  (e as any).held = p;
  console.log((e as any).message);
}
console.log(p.a);
"#;

#[test]
fn a_caught_runtime_error_is_the_runtime_error_object() {
    let pf = facts(FIXTURE);
    let dump = pf.dump_text();
    let (top, rs) = recv(&pf, "message");
    assert!(!top, "catch receiver is ⊤:\n{dump}");
    assert!(
        has(&rs, "runtime:Error"),
        "no runtime error in {rs:?}:\n{dump}"
    );
    assert!(tmpl(&pf, "P").escape.is_none(), "P escaped:\n{dump}");
    assert!(
        matches!(site(&pf, "a").verdict, Verdict::T0Mono | Verdict::T0Set),
        "p.a unproven:\n{dump}"
    );
}

/// ⊤ elsewhere in the program (a call of an unknown value outside the try)
/// does not reach a catch whose try body runs only known code: it receives
/// what its callee throws and the runtime error. Red on the global rule.
#[test]
fn a_catch_around_known_code_holds_no_top() {
    let pf = facts(&unseen(
        r#"
declare const ext: any;
class Q { b = 2; }
function f(n: number) { if (n > 0) throw new Q(); return n; }
ext(1);
try { f(process.argv.length); } catch (e) { console.log((e as any).b); }
"#,
    ));
    let (top, rs) = recv(&pf, "b");
    assert!(!top, "catch receiver is ⊤: {rs:?}\n{}", pf.dump_text());
    assert!(
        has(&rs, "Q") && has(&rs, "runtime:Error"),
        "{rs:?}\n{}",
        pf.dump_text()
    );
}

/// A program throw reaches the catch through the call that runs it (the bind
/// edge, callee region -> call region), also two calls deep.
#[test]
fn a_program_throw_is_still_caught() {
    let pf = facts(
        r#"
class Q { b = 2; }
function f(n: number) { if (n > 0) throw new Q(); return n; }
function g(n: number) { return f(n) + 1; }
try { g(process.argv.length); } catch (e) { console.log((e as any).b); }
"#,
    );
    let (top, rs) = recv(&pf, "b");
    assert!(
        !top && has(&rs, "Q") && has(&rs, "runtime:Error"),
        "{rs:?}\n{}",
        pf.dump_text()
    );
}

/// Named control: a try body that runs unknown code, directly or through a
/// function it calls, may throw anything: the caught value is ⊤.
#[test]
fn top_in_the_try_reaches_the_catch() {
    let pf = facts(&unseen(
        r#"
declare const ext: any;
try { ext(); } catch (e) { console.log((e as any).c); }
"#,
    ));
    let (top, _) = recv(&pf, "c");
    assert!(top, "{}", pf.dump_text());
    let pf = facts(&unseen(
        r#"
declare const ext: any;
function h() { ext(); }
try { h(); } catch (e) { console.log((e as any).d); }
"#,
    ));
    let (top, _) = recv(&pf, "d");
    assert!(top, "{}", pf.dump_text());
}

fn transformed(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

/// Passes: a generator's `throw(v)` is caught as it is: a sync generator's
/// around the call (its region), an async one's where its rejection is
/// awaited (the shared cell).
#[test]
fn a_generator_throw_is_caught() {
    let pf = transformed(
        r#"
class R { r = 3; }
function* g() { yield 1; }
const it = g();
it.next();
try { it.throw(new R()); } catch (e) { console.log((e as any).r); }
"#,
    );
    let (top, rs) = recv(&pf, "r");
    assert!(!top && has(&rs, "R"), "{rs:?}\n{}", pf.dump_text());
    let pf = transformed(
        r#"
class R2 { r2 = 3; }
async function* g() { yield 1; }
const it = g();
async function main() {
  await it.next();
  try { await it.throw(new R2()); } catch (e) { console.log((e as any).r2); }
}
main();
"#,
    );
    let (top, rs) = recv(&pf, "r2");
    assert!(top || has(&rs, "R2"), "{rs:?}\n{}", pf.dump_text());
    eprintln!("async generator catch: top={top} {rs:?}");
}

/// Passes: `emit("error", er)` with no listener throws `er`.
#[test]
fn an_emitted_error_is_caught() {
    let pf = facts(
        r#"
import { EventEmitter } from "events";
class S { s = 4; }
const em = new EventEmitter();
try { em.emit("error", new S()); } catch (e) { console.log((e as any).s); }
"#,
    );
    let (top, rs) = recv(&pf, "s");
    assert!(top || has(&rs, "S"), "{rs:?}\n{}", pf.dump_text());
    eprintln!("emit catch: top={top} {rs:?}");
}

/// Passes: Promise.any rejects with an AggregateError whose `errors` are the
/// reasons (through the shared cell, which every catch reads).
#[test]
fn promise_any_rejects_with_the_reasons() {
    let pf = facts(
        r#"
declare const ext: any;
ext(4);
class T { t = 5; }
async function main() {
  try { await Promise.any([Promise.reject(new T())]); }
  catch (e) { console.log((e as any).errors[0].t); }
}
main();
"#,
    );
    let (_, rs) = recv(&pf, "t");
    assert!(has(&rs, "T"), "{rs:?}\n{}", pf.dump_text());
}

/// A coercion runs the program's `toString`: what it throws reaches the
/// catch around the template literal and around `String(x)`.
#[test]
fn a_throwing_to_string_is_caught() {
    for (expr, key) in [("`${v}`", "u"), ("String(v)", "w"), ("\"\" + v", "y")] {
        let src = format!(
            r#"
declare const ext: any;
ext(5);
class U {{ {key} = 6; }}
class V {{ toString(): string {{ throw new U(); }} }}
const v = new V();
try {{ console.log({expr}); }} catch (e) {{ console.log((e as any).{key}); }}
"#
        );
        let pf = facts(&unseen(&src));
        let (top, rs) = recv(&pf, key);
        assert!(top || has(&rs, "U"), "{expr}: {rs:?}\n{}", pf.dump_text());
        eprintln!("{expr}: top={top} {rs:?}");
    }
}

/// One body run by two callers with different arguments (its summary bound
/// twice): each catch receives what its call throws.
#[test]
fn a_shared_callee_throws_into_each_callers_catch() {
    let pf = facts(
        r#"
class A { a = 1; }
class B { bb = 2; }
function thr(x: any) { if (process.argv.length > 0) throw x; return 0; }
function mk(n: number) { if (n > 0) throw new A(); return n; }
try { thr(new A()); } catch (e) { console.log((e as any).a); }
try { thr(new B()); } catch (e) { console.log((e as any).bb); }
try { mk(process.argv.length); } catch (e) { console.log((e as any).a); }
"#,
    );
    for s in pf.sites.iter().filter(|s| s.key == "a" || s.key == "bb") {
        let want = if s.key == "a" { "A" } else { "B" };
        assert!(
            s.top || has(&s.receivers, want),
            "{} {:?}\n{}",
            s.key,
            s.receivers,
            pf.dump_text()
        );
    }
}

/// Unknown code holds the callback, so it may call it and catch what it
/// throws: the thrown `Q` reaches unknown code (node: a host
/// `ext(cb) { try { cb() } catch (e) { leak(e) } }`), and every `Q` is
/// unknown code's to read. The escaped function's region feeds the escape
/// sink (ESCRAISE). Red without that edge.
#[test]
fn a_throw_from_a_callback_unknown_code_holds_escapes() {
    let pf = facts(&unseen(
        r#"
declare const ext: any;
class Q { b = 2; }
ext(() => { throw new Q(); });
const q = new Q();
console.log(q.b);
"#,
    ));
    assert!(
        tmpl(&pf, "Q").escape.is_some(),
        "Q did not escape:\n{}",
        pf.dump_text()
    );
}

/// A getter runs on a program load: what it throws reaches the catch
/// around that load (node: prints 2).
#[test]
fn a_getter_throw_is_caught() {
    let pf = facts(
        r#"
class Q { b = 2; }
const o = { get x(): number { if (process.argv.length > 0) throw new Q(); return 1; } };
try { console.log(o.x); } catch (e) { console.log((e as any).b); }
"#,
    );
    let (top, rs) = recv(&pf, "b");
    assert!(top || has(&rs, "Q"), "{rs:?}\n{}", pf.dump_text());
}

/// Unknown code holds an object with a throwing getter: it may read the
/// key and catch the throw, so the thrown `Q` escapes.
#[test]
fn a_getter_throw_on_an_escaped_object_escapes() {
    let pf = facts(&unseen(
        r#"
declare const ext: any;
class Q { b = 2; }
ext({ get x(): number { throw new Q(); } });
const q = new Q();
console.log(q.b);
"#,
    ));
    assert!(
        tmpl(&pf, "Q").escape.is_some(),
        "Q did not escape:\n{}",
        pf.dump_text()
    );
}
