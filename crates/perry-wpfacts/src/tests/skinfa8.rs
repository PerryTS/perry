//! A call a native makes while it runs for a PROGRAM call is that run's own
//! call (its own `this`, arguments, result and callee constraint), as the
//! program call site makes one call per callee it reaches; only the calls
//! made below such a root-level call share one call per (root-level call,
//! code position, discriminator, counts) (`Model::chain_call`).
//!
//! `Promise.all([e0, e1])` adopts item 0 into the settled slot of index 0
//! and item 1 into that of index 1 (27.2.4.1.2): two runs of `adopt` for
//! one program call, each calling `ei.then(resolve_i, reject)`
//! (NewPromiseResolveThenableJob, 27.2.2.2). One shared call for both
//! passed `resolve_0` and item 0's promise to item 1's `then`. The tests
//! here keep the SKINFA4-6 recursions ending under the per-run key.

use super::*;

fn with_index<T>(f: impl FnOnce() -> T) -> T {
    crate::provenance::with_env(&[("PERRY_FACTS_PROMISE_ALL_INDEX", "1")], f)
}

fn src(tail: &str) -> String {
    format!(
        r#"
        declare function external(x?: unknown): any;
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const s = new Svc();
        const h: any = external();
        {tail}
        "#
    )
}

/// A solve that must end (no cap): `f` runs on its own result.
fn ends(tail: &'static str) -> Verdict {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let v = with_index(|| site(&facts(&src(tail)), "size").verdict.clone());
            let _ = tx.send(v);
        })
        .unwrap();
    rx.recv_timeout(std::time::Duration::from_secs(120))
        .expect("the solve ends")
}

/// node: prints `[object Array]`-free text and `3`: `join` is
/// Array.prototype.toString itself, which invokes `join` again on the
/// array (SKINFA4/5 member-call recursion).
#[test]
fn keep_to_string_join_recursion_ends() {
    let v = ends(
        "const a: any = [[s, [s]], [s]]; a.join = Array.prototype.toString; \
         a[0].join = Array.prototype.toString; console.log(String(a).length, g(s));",
    );
    let _ = v;
}

/// node: never settles, prints `3`: a thenable whose `then` resolves with
/// itself (SKINFA6 then / adopt cycle).
#[test]
fn keep_self_resolving_thenable_ends() {
    let v = ends(
        "const t: any = { then(res: any) { res(t); } }; \
         Promise.resolve(t).then((x: any) => x); console.log(g(s));",
    );
    let _ = v;
}
