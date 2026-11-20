//! A generator object's [[Prototype]] is its generator function's
//! `prototype` object, an ordinary object the program may write. A member
//! the program stored there (or that unknown code may have stored there)
//! is what a lookup on the generator object finds before the generator
//! protocol: `for (… of g())` and `it.next()` call it.

use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        class Alt {{ other = 1; size = 4; }}
        function g(a: any) {{ return a.size; }}
        function* gen() {{ yield new Svc(); }}
        {src}
        "#
    );
    let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

fn receivers(src: &str) -> (Vec<String>, Verdict) {
    let pf = facts_of(src);
    let s = site(&pf, "size");
    (s.receivers.clone(), s.verdict.clone())
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// Control. node: prints `3`. Nothing is written on the chain: the site
/// reads the yielded `Svc` alone.
#[test]
fn without_a_written_member_the_site_is_proven() {
    let (r, v) = receivers("for (const s of gen()) { console.log(g(s)); break; }");
    assert!(proven(&v), "{r:?} {v:?}");
    assert!(!r.iter().any(|r| r.contains("Alt")), "{r:?}");
}

/// node: prints `4`. for-of calls `next` through the chain: the written
/// method's result value reaches the site.
#[test]
fn for_of_calls_a_next_written_on_the_prototype() {
    let (r, v) = receivers(
        "gen.prototype.next = function () { return { value: new Alt(), done: false }; };
         for (const s of gen()) { console.log(g(s)); break; }",
    );
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
}

/// node: prints `4`. A plain `next()` call finds the written method too.
#[test]
fn next_call_finds_a_next_written_on_the_prototype() {
    let (r, v) = receivers(
        "gen.prototype.next = function () { return { value: new Alt(), done: false }; };
         console.log(g(gen().next().value));",
    );
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
}

/// node: prints `4`. for-of starts at `[Symbol.iterator]`, which the
/// program replaced on the generator's prototype.
#[test]
fn for_of_calls_an_iterator_method_written_on_the_prototype() {
    let (r, v) = receivers(
        "gen.prototype[Symbol.iterator] = function () {
             return { next() { return { value: new Alt(), done: false }; } };
         };
         for (const s of gen()) { console.log(g(s)); break; }",
    );
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
}

/// Unknown code holds the generator function: it may write
/// `gen.prototype.next`, so the site is not proven.
#[test]
fn an_escaped_generator_function_leaves_the_site_unproven() {
    let (r, v) = receivers(
        "(globalThis as any).hook(gen);
         for (const s of gen()) { console.log(g(s)); break; }",
    );
    assert!(!proven(&v), "{r:?} {v:?}");
}

/// node: prints `4`. A step closure that captures the generator's
/// parameter has one function token per closure context; each of them
/// inherits from the generator function's `prototype`.
#[test]
fn a_capturing_step_finds_a_next_written_on_the_prototype() {
    let (r, v) = receivers(
        "function* gp(n: number) { const s = new Svc(); s.size = n; yield s; }
         gp.prototype.next = function () { return { value: new Alt(), done: false }; };
         for (const s of gp(process.argv.length + 1)) { console.log(g(s)); break; }",
    );
    assert!(r.iter().any(|r| r.contains("Alt")), "{r:?} {v:?}");
}
