//! GLOB3: seeds that hold the realm in the app variant, one exact model each.
//! Seed 1: the async step driver settles its promise with a `reject` call on
//! the global sentinel, which codegen lowers to %Promise%.reject (ECMA-262
//! 27.2.4.6); it reads no global binding.
use super::*;

/// The facts of `src` after the async and generator transforms (as the
/// compile pipeline runs them before the solve).
fn facts_of(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_async_to_generator(&mut hir);
    perry_transform::transform_generators(&mut hir);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The verdict of the first site `.key` after `marker`, and the dump.
fn verdict(src: &str, key: &str, marker: &str) -> (Verdict, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    (s.verdict.clone(), pf.dump_text())
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// RED: an async function with an `await` (its step driver's reject call)
/// does not hand the program's objects to unknown code: a later builtin
/// call's argument stays proven.
#[test]
fn gl3_s1_async_step_reject_is_promise_reject() {
    let src = "class N { v = 1; }
        const n = new N();
        async function f(x: number) { await null; return x + 1; }
        f(Number(process.argv[2])).then((r) => console.log(r));
        [n].forEach((x) => console.log(x.v));
        console.log(/*A*/ n.v);";
    let (v, dump) = verdict(src, "v", "/*A*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
}

/// RED: the value an async function throws reaches the `catch` handler
/// through the step driver's reject: the handler's read of it is proven.
#[test]
fn gl3_s1_async_throw_reaches_catch() {
    let src = "class E { v = 2; }
        async function f(s: string) { await null; if (s === \"x\") throw new E(); return 1; }
        f(process.argv[2]).catch((e: E) => console.log(/*B*/ e.v));";
    let (v, dump) = verdict(src, "v", "/*B*/");
    assert!(proven(&v), "e.v proven, got {v:?}\n{dump}");
}

/// KEEP: `Promise.reject` with `this` a program subclass of Promise:
/// NewPromiseCapability(C) constructs C (27.2.4.6 step 2): the subclass
/// constructor is program code and sees its executor; the handler reads
/// the argument. Must stay sound (no claim that the subclass is skipped).
#[test]
fn gl3_s1_keep_subclass_reject() {
    let src = "class N { v = 3; }
        class P<T> extends Promise<T> { tag = 1; }
        const n = new N();
        const p = P.reject(process.argv[2] === \"x\" ? n : n);
        p.catch((e: N) => console.log(/*C*/ e.v));";
    let pf = facts_of(src);
    let at = src.find("/*C*/").unwrap() as u32;
    let s = pf
        .sites
        .iter()
        .find(|s| s.key == "v" && s.byte_offset >= at);
    // Either proven with the argument as the rejection value, or refused:
    // never a verdict whose receiver set lacks N.
    if let Some(s) = s {
        if let Verdict::No(_) = s.verdict {
        } else {
            let d = format!("{:?}", s.verdict);
            assert!(
                !d.contains("Dead"),
                "the handler is live: {d}\n{}",
                pf.dump_text()
            );
        }
    }
}

/// KEEP: a program `Promise.reject(x)` whose rejection is never handled:
/// the argument is not lost (an unhandled rejection is reported by the
/// runtime, which prints it): no site of it becomes Dead.
#[test]
fn gl3_s1_keep_unhandled_rejection() {
    let src = "class N { v = 4; }
        const n = new N();
        async function f() { await null; Promise.reject(n); }
        f();
        console.log(/*D*/ n.v);";
    let (v, dump) = verdict(src, "v", "/*D*/");
    assert!(
        !format!("{v:?}").contains("Dead"),
        "n is live, got {v:?}\n{dump}"
    );
}

/// KEEP: the rejection handler is program code and is called with the
/// argument: a store it makes on the argument is seen by a later read.
#[test]
fn gl3_s1_keep_handler_sees_argument() {
    let src = "class N { v = 5; w = 0; }
        const n = new N();
        Promise.reject(n).catch((e: N) => { e.w = Number(process.argv[2]); });
        setTimeout(() => console.log(/*E*/ n.w), 0);";
    let (v, dump) = verdict(src, "w", "/*E*/");
    assert!(
        !format!("{v:?}").contains("Dead"),
        "n.w live, got {v:?}\n{dump}"
    );
}
