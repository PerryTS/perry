//! Regression tests for promise reaction-slot clobbering.
//!
//! A promise carries ONE inline reaction slot (`on_fulfilled` /
//! `on_rejected` / `next`). `js_promise_then` handles multi-reaction
//! promises (first reaction takes the slot, 2nd+ divert to the overflow
//! table), but two other attach paths stored into the slot UNCONDITIONALLY,
//! destroying whatever reaction was already registered:
//!
//! 1. `js_promise_attach_handlers` — used by Promise.all/allSettled/race/any
//!    (via `promise_resolve_for_combinator`, which returns the USER promise
//!    identity) and the stream adapters. `p.then(cb); Promise.all([p])` on a
//!    pending `p` overwrote `cb`; two combinators sharing one pending input
//!    destroyed each other's forwarder, so the loser's remaining-count never
//!    reached zero — a permanent hang. Webpack/Turbopack chunk promises are
//!    shared constants joined via Promise.all in multiple module factories.
//!
//! 2. `js_promise_finally` on a PENDING promise — overwrote the slot AND
//!    nulled `promise.next`, so `p.then(cb); p.finally(end)` lost `cb` and
//!    its chained promise never settled.
//!
//! (The SETTLED-promise `.finally` variant — N settled finallys ran the last
//! wrapper N times — is covered by the settled-arm fix carried in the same
//! PR and guarded by the last test.)
//!
//! All expected outputs are byte-for-byte what `node
//! --experimental-strip-types` prints.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// A user `.then` attached BEFORE Promise.all on the same pending promise:
/// both reactions must fire. Pre-fix the combinator's forwarder overwrote the
/// `.then` callback (it never ran).
#[test]
fn then_plus_promise_all_on_shared_pending_promise() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let resolveP: any;
const p = new Promise((r) => { resolveP = r; });
p.then((v) => console.log("then:", v));
Promise.all([p]).then(([v]) => console.log("all:", v));
resolveP(1);
"#,
    );
    assert_eq!(
        stdout, "then: 1\nall: 1\n",
        "a combinator must not clobber a previously attached .then reaction"
    );
}

/// Two combinators sharing ONE pending promise: both must settle. Pre-fix the
/// second combinator's forwarder destroyed the first one's, so the first
/// combinator's result promise never settled (hang → the binary would only
/// print the surviving line, or nothing).
#[test]
fn two_combinators_on_shared_pending_promise() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let resolveQ: any;
const q = new Promise((r) => { resolveQ = r; });
Promise.all([q]).then(([v]) => console.log("q-all:", v));
Promise.race([q]).then((v) => console.log("q-race:", v));
resolveQ(2);
"#,
    );
    assert_eq!(
        stdout, "q-all: 2\nq-race: 2\n",
        "two combinators sharing one pending promise must both settle"
    );
}

/// `.then` chain plus `.finally` chain on one pending promise. Pre-fix the
/// pending-arm `.finally` overwrote the `.then` wrapper AND nulled the
/// chained-promise link, so "then:"/"then-chain" never printed. Ordering
/// (finally's extra microtask tick) must match node.
#[test]
fn then_plus_finally_on_pending_promise() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let resolveR: any;
const r = new Promise((res) => { resolveR = res; });
r.then((v) => console.log("then:", v)).then(() => console.log("then-chain"));
r.finally(() => console.log("finally")).then((v) => console.log("finally-chain:", v));
resolveR(3);
"#,
    );
    assert_eq!(
        stdout, "then: 3\nfinally\nthen-chain\nfinally-chain: 3\n",
        ".finally on a pending promise must not clobber the earlier .then \
         reaction or its chain"
    );
}

/// Two `.finally`s registered while pending, then a `.then`: each callback
/// must run exactly once. Pre-fix the second finally overwrote the first
/// (f1 never ran).
#[test]
fn two_finallys_on_pending_promise() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let resolveS: any;
const s = new Promise((res) => { resolveS = res; });
s.finally(() => console.log("f1"));
s.finally(() => console.log("f2"));
s.then((v) => console.log("s-then:", v));
resolveS(4);
"#,
    );
    assert_eq!(
        stdout, "f1\nf2\ns-then: 4\n",
        "each pending .finally must run exactly once, in registration order"
    );
}

/// `catch` + `finally` on a pending promise that REJECTS: the handler runs,
/// cleanup runs, and the rejection counts as handled (no unhandled-rejection
/// noise, exit 0). The `.finally` chain gets its own `.catch` because
/// `.finally` re-rejects into its returned promise — node exits 1 with
/// ERR_UNHANDLED_REJECTION otherwise (and perry matches that behavior).
#[test]
fn catch_plus_finally_on_pending_rejection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let rejectU: any;
const u = new Promise((_res, rej) => { rejectU = rej; });
u.catch((e) => console.log("caught:", e));
u.finally(() => console.log("cleanup")).catch(() => {});
rejectU("boom");
"#,
    );
    assert_eq!(
        stdout, "caught: boom\ncleanup\n",
        ".finally must not clobber a .catch on a pending promise"
    );
}

/// Settled-arm guard: two `.finally`s on an ALREADY-settled promise must each
/// run exactly once (previously the last wrapper ran N times, the others
/// never — the Turbopack shared pre-fulfilled chunk-promise shape).
#[test]
fn two_finallys_on_settled_promise() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const t = Promise.resolve(7);
t.finally(() => console.log("g1"));
t.finally(() => console.log("g2"));
t.then((v) => console.log("then:", v));
"#,
    );
    assert_eq!(
        stdout, "g1\ng2\nthen: 7\n",
        "each settled .finally must dispatch its own wrapper exactly once"
    );
}

/// A degenerate no-arg `p.then()` parks in the slot with BOTH handler
/// fields null and only `next` set. The occupancy check must treat that
/// `next` as occupied — otherwise a later combinator stores its handlers
/// beside it and the pass-through chain resolves with the handler's return
/// instead of the value (CodeRabbit finding on the initial version of this
/// fix).
#[test]
fn degenerate_then_chain_survives_combinator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let resolveP: any;
const p = new Promise((r) => { resolveP = r; });
const chain = p.then();
chain.then((v) => console.log("chain:", v));
Promise.all([p]).then(([v]) => console.log("all:", v));
resolveP(9);
"#,
    );
    assert_eq!(
        stdout, "chain: 9\nall: 9\n",
        "a bare p.then() pass-through chain must survive a later combinator"
    );
}
