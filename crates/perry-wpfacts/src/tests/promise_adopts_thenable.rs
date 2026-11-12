//! A promise is never fulfilled with a thenable (ECMA-262 27.2.1.3.2
//! Promise Resolve Functions steps 7-15, 27.2.2.1
//! NewPromiseResolveThenableJob): a value whose `then` read is callable is
//! adopted (a native promise passes on its settled value, a program
//! thenable's `then` is called with fresh resolve/reject functions); a
//! value whose `then` read holds no function fulfils as itself. The same
//! rule holds for a resolve function, a then-callback's result,
//! `Promise.resolve` and `await`.
use super::*;

fn sites(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn mono(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::T0Mono))
}

fn open(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().any(|v| matches!(v, Verdict::No(_)))
}

const T: &str = "class T { m() { return 1; } }\nclass U { m() { return 2; } }\n";

fn run(src: &str) -> ProgramFacts {
    facts(&format!("{T}{src}"))
}

/// A then-callback returns a promise: the next callback gets its value.
#[test]
fn then_callback_promise_is_adopted() {
    let pf = run(r#"
        Promise.resolve(1)
            .then(() => new Promise<T>((r) => r(new T())))
            .then((t: any) => console.log(t.m()));
    "#);
    let v = sites(&pf, "m", true);
    assert!(mono(&v), "{v:?}\n{}", pf.dump_text());
}

/// A resolve function given a promise adopts it.
#[test]
fn resolver_adopts_a_promise() {
    let pf = run(r#"
        new Promise<any>((res) => res(new Promise<T>((r) => r(new T()))))
            .then((t: any) => console.log(t.m()));
    "#);
    let v = sites(&pf, "m", true);
    assert!(mono(&v), "{v:?}\n{}", pf.dump_text());
}

/// `await` of an async function that returns a promise.
#[test]
fn await_of_returned_promise_is_its_value() {
    let pf = run(r#"
        async function f(): Promise<any> { return new Promise<T>((r) => r(new T())); }
        async function g() { const t: any = await f(); console.log(t.m()); }
        g();
    "#);
    let v = sites(&pf, "m", true);
    assert!(mono(&v), "{v:?}\n{}", pf.dump_text());
}

/// A program thenable: its `then` runs with resolve functions, the next
/// callback gets what it resolves, not the thenable.
#[test]
fn program_thenable_runs_its_then() {
    let pf = run(r#"
        const th: any = { then(settle: any) { settle(new U()); } };
        Promise.resolve(1)
            .then(() => th)
            .then((u: any) => console.log(u.m()));
    "#);
    let v = sites(&pf, "m", true);
    assert!(mono(&v), "{v:?}\n{}", pf.dump_text());
    // `settle(new U())` runs: its callee is the runtime's resolve function.
    let s = sites(&pf, "()", true);
    assert!(
        s.len() == 1 && format!("{s:?}").contains("runtime function"),
        "settle {s:?}\n{}",
        pf.dump_text()
    );
}

/// A data property `then: 5` is no function: the object fulfils as itself.
#[test]
fn noncallable_then_fulfils_with_itself() {
    let pf = run(r#"
        Promise.resolve(1)
            .then(() => ({ then: 5, k() { return 3; } }))
            .then((o: any) => console.log(o.k()));
    "#);
    let v = sites(&pf, "k", true);
    assert!(mono(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control: an own `then` of unknown code on the inner promise.
#[test]
fn unknown_own_then_stays_open() {
    let pf = run(r#"
        Promise.resolve(1)
            .then(() => {
                const p: any = new Promise<T>((r) => r(new T()));
                p.then = new Function(process.argv[2]);
                return p;
            })
            .then((t: any) => console.log(t.m()));
    "#);
    let v = sites(&pf, "m", true);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control: an executor of unknown code.
#[test]
fn unknown_executor_stays_open() {
    let pf = run(r#"
        new Promise<any>(new Function(process.argv[2]) as any)
            .then((t: any) => console.log(t.m()));
    "#);
    let v = sites(&pf, "m", true);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control: an unknown value returned from the callback.
#[test]
fn unknown_value_stays_open() {
    let pf = run(r#"
        Promise.resolve(1)
            .then(() => (new Function(process.argv[2]) as any)())
            .then((t: any) => console.log(t.m()));
    "#);
    let v = sites(&pf, "m", true);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}
