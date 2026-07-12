//! #6084 item 2: settling a promise must not cost O(table) in the promise
//! reaction side tables.
//!
//! `js_promise_resolve`/`reject` unconditionally drain three promise-pointer-keyed
//! side tables — `PROMISE_SETTLE_LISTENERS`, `PROMISE_OVERFLOW_REACTIONS` and
//! `PROMISE_ALL_STATES`. All three were `Vec<(usize, T)>` scanned END TO END on
//! every settle, so settling N promises that are parked in a table is O(N²).
//! Measured on `main` (2nd `.then` per promise, so each parks an overflow
//! reaction), settle time for N promises: 5k=18ms, 10k=73ms, 20k=779ms,
//! 40k=3503ms — against node's ~1ms. They are now backed by `PromiseKeyedTable`
//! (dense `Vec` for the GC scanners + an O(1) key index), so a settle drains
//! only its own key's entries.
//!
//! Pinned as a SCALING check rather than an absolute time: 4x the promises must
//! not cost ~16x the time. The margin is wide enough for a loaded CI host but
//! still fails the quadratic shape by a mile (on `main` the 40k/10k ratio is
//! ~48x, and the absolute numbers are seconds).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write entry");
    let output = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(
        run.status.success(),
        "binary failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    stdout
}

/// Overflow reactions (2nd+ `.then` on a pending promise) — the table that
/// dominated the measured regression.
#[test]
fn settling_many_promises_with_overflow_reactions_is_not_quadratic() {
    let stdout = compile_and_run(
        r#"
function runOne(N: number): Promise<number> {
  const resolvers: ((v: number) => void)[] = []
  const ps: Promise<number>[] = []
  for (let i = 0; i < N; i++) {
    ps.push(new Promise<number>((res) => { resolvers.push(res) }))
  }
  let sum = 0
  const tails: Promise<void>[] = []
  for (const p of ps) {
    p.then(() => {})                                  // inline reaction slot
    tails.push(p.then((v: number) => { sum += v }))   // -> overflow table
  }
  const done = Promise.all(tails)
  const t0 = Date.now()
  for (let i = 0; i < N; i++) resolvers[i](1)
  const settleMs = Date.now() - t0
  return done.then(() => {
    if (sum !== N) throw new Error("lost reactions: sum=" + sum + " want " + N)
    return settleMs
  })
}

// Warm, then compare 10k against 40k (4x the promises).
runOne(2000).then(() => runOne(10000)).then((small: number) => {
  runOne(40000).then((large: number) => {
    console.log("small_ms=" + small + " large_ms=" + large)
    // Linear => ~4x. Quadratic => ~16x (main: 73ms -> 3503ms = 48x).
    const quadratic = small >= 4 && large > small * 10
    console.log("QUADRATIC:" + quadratic)
    console.log("DONE")
  })
})
"#,
    );

    assert!(
        stdout.contains("QUADRATIC:false") && stdout.contains("DONE"),
        "settle scaling looks quadratic:\n{stdout}"
    );
}

/// Reaction ORDER is observable and must survive the switch from a linear
/// order-preserving scan to the keyed table (which restores FIFO from each
/// entry's registration seq). Also covers a promise parked in several
/// `Promise.all` calls at once, plus rejection paths.
#[test]
fn reaction_order_and_multi_registration_are_preserved() {
    let stdout = compile_and_run(
        r#"
const order: string[] = []

// Many reactions on ONE promise: 1st uses the inline slot, the rest overflow.
// They must replay in registration (FIFO) order.
let resolveShared: (v: number) => void = () => {}
const shared = new Promise<number>((res) => { resolveShared = res })
for (let i = 0; i < 6; i++) {
  const label = "r" + i
  shared.then(() => { order.push(label) })
}

// The same pending promise feeding two Promise.all calls.
let resolveOther: (v: number) => void = () => {}
const other = new Promise<number>((res) => { resolveOther = res })
const allA = Promise.all([shared, other])
const allB = Promise.all([shared, Promise.resolve(9)])

// Rejection side: catch must still fire for overflow reactions. (The derived
// promise of the first `.then` rejects, so give it a handler — an unhandled
// rejection would abort the process, not exercise the table.)
let rejectMe: (e: any) => void = () => {}
const bad = new Promise<number>((_res, rej) => { rejectMe = rej })
bad.then(() => { order.push("bad-then") }).catch(() => {})
bad.catch(() => { order.push("bad-catch") })

resolveShared(1)
resolveOther(2)
rejectMe(new Error("x"))

Promise.all([allA, allB]).then((rs: number[][]) => {
  console.log("order:" + order.join(","))
  console.log("allA:" + rs[0].join(","))
  console.log("allB:" + rs[1].join(","))
  console.log("DONE")
})
"#,
    );

    for needle in [
        "order:r0,r1,r2,r3,r4,r5,bad-catch",
        "allA:1,2",
        "allB:1,9",
        "DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
