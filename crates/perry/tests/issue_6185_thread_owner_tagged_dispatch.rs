//! #6185 Tier 2 (end-to-end): owner-tagged cross-thread dispatch must not
//! disturb any of the shipped `perry/thread` / timer behavior.
//!
//! Tier 2 tags every entry on the three process-global queues
//! (`PENDING_THREAD_RESULTS`, `TIMER_QUEUE` / `CALLBACK_TIMERS` /
//! `INTERVAL_TIMERS`) with the agent that owns the heap its pointers live in,
//! and makes every drain skip entries it does not own. The unsound drains it
//! removes are not directly expressible in TypeScript anymore — Tier 1 (#6276)
//! rejects the `await`-inside-a-worker that used to reach them — so the property
//! under test here is the *other* half: the queues still deliver everything they
//! are supposed to, on the thread that owns them.
//!
//! The owner-filtering itself (a worker neither firing nor eating a foreign
//! agent's timer; a retired worker's timers being purged) is asserted directly
//! against the runtime in `perry-runtime/src/agent_dispatch_tests.rs`.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The failure mode for most of these is a HANG, not a wrong answer: an
/// owner-tag regression files a result under the wrong agent, so the promise it
/// belongs to never settles and the program spins its event loop forever.
/// `cargo test` has no per-test timeout and `Command::output()` blocks
/// indefinitely, so an unbounded run would stall the whole suite with no
/// diagnostic. Bound it and report the hang as a normal test failure instead.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

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

    // Piped stdio + poll, so a program that never settles is killed and
    // reported rather than blocking the harness forever. These programs print a
    // few dozen bytes, so they cannot fill the pipe buffer while we poll.
    let mut child = Command::new(&output)
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");

    let deadline = Instant::now() + RUN_TIMEOUT;
    loop {
        match child.try_wait().expect("poll compiled binary") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let out = child.wait_with_output().expect("reap timed-out binary");
                panic!(
                    "compiled binary did not exit within {:?} — a promise never settled \
                     (owner-tag regression: work filed under an agent that never drains it)\
                     \nstdout so far:\n{}\nstderr so far:\n{}",
                    RUN_TIMEOUT,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }

    let run = child.wait_with_output().expect("reap compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// `spawn`'s completion still resolves on the agent that spawned it. This is the
/// queue whose drain was previously unconditional: the result is tagged with the
/// *spawning* agent (captured on the spawning thread, not inside the worker), so
/// the main thread — and only the main thread — settles the promise.
#[test]
fn spawn_result_still_resolves_on_the_spawning_agent() {
    let out = compile_and_run(
        r#"
import { spawn } from "perry/thread";
async function main(): Promise<void> {
  const result = await spawn(() => {
    let sum = 0;
    for (let i = 1; i <= 100; i++) sum += i;
    return sum;
  });
  console.log("spawn:" + result);
}
main();
"#,
    );
    assert_eq!(out.trim(), "spawn:5050");
}

/// Many concurrent `spawn`s settle in the right order with the right values —
/// the owner-filtered drain is an order-preserving partition, not a
/// `swap_remove` filter (which would shuffle the settle order).
#[test]
fn concurrent_spawn_results_all_settle_in_order() {
    let out = compile_and_run(
        r#"
import { spawn } from "perry/thread";
async function main(): Promise<void> {
  const jobs = [1, 2, 3, 4, 5, 6, 7, 8].map((n) =>
    spawn(() => {
      let acc = 0;
      for (let i = 0; i < 1000; i++) acc += n;
      return acc;
    })
  );
  const results = await Promise.all(jobs);
  console.log(results.join(","));
}
main();
"#,
    );
    assert_eq!(out.trim(), "1000,2000,3000,4000,5000,6000,7000,8000");
}

/// `parallelMap` / `parallelFilter` workers each claim (and retire) their own
/// agent. Their results must be unaffected and order-preserved.
#[test]
fn parallel_map_and_filter_still_work_across_worker_agents() {
    let out = compile_and_run(
        r#"
import { parallelMap, parallelFilter } from "perry/thread";
const nums = [1, 2, 3, 4, 5, 6, 7, 8];
const doubled = parallelMap(nums, (x: number) => x * 2);
const evens = parallelFilter(nums, (x: number) => x % 2 === 0);
console.log("map:" + doubled.join(","));
console.log("filter:" + evens.join(","));
"#,
    );
    assert_eq!(out.trim(), "map:2,4,6,8,10,12,14,16\nfilter:2,4,6,8");
}

/// The main agent's timers, intervals, immediates and microtasks all still fire,
/// in spec order, now that every tick is owner-filtered. A regression to a raw
/// `ThreadId` comparison (or a mis-tagged primary agent) shows up here as
/// missing output / a hang.
#[test]
fn main_agent_timers_intervals_and_microtasks_all_still_fire_in_order() {
    let out = compile_and_run(
        r#"
const order: string[] = [];
setTimeout(() => order.push("t100"), 100);
setTimeout(() => order.push("t0"), 0);
setImmediate(() => order.push("immediate"));
Promise.resolve().then(() => order.push("microtask"));
queueMicrotask(() => order.push("qmt"));
let n = 0;
const iv = setInterval(() => {
  n++;
  order.push("interval" + n);
  if (n === 3) clearInterval(iv);
}, 20);
const cancelled = setTimeout(() => order.push("SHOULD-NOT-FIRE"), 30);
clearTimeout(cancelled);
await new Promise<void>((r) => setTimeout(r, 250));
console.log(order.join(","));
"#,
    );
    // Same ordering `node --experimental-strip-types` produces for this program.
    assert_eq!(
        out.trim(),
        "microtask,qmt,t0,immediate,interval1,interval2,interval3,t100"
    );
}

/// Timers scheduled while worker agents are alive still belong to — and fire on
/// — the main agent. The worker's own tick (had it run one) would skip them, and
/// retiring the worker must not purge them: `retire_agent` keys on the owner tag,
/// so only the worker's own entries go.
#[test]
fn worker_lifetime_does_not_disturb_the_main_agents_timers() {
    let out = compile_and_run(
        r#"
import { spawn, parallelMap } from "perry/thread";
async function main(): Promise<void> {
  const fired: string[] = [];
  setTimeout(() => fired.push("before-worker"), 30);

  // Workers come and go (each claims an agent, then retires it) while the
  // main agent's timers are pending.
  const doubled = parallelMap([1, 2, 3], (x: number) => x * 2);
  const sum = await spawn(() => 7 * 6);

  setTimeout(() => fired.push("after-worker"), 60);
  await new Promise<void>((r) => setTimeout(r, 200));

  console.log("map:" + doubled.join(","));
  console.log("spawn:" + sum);
  console.log("timers:" + fired.join(","));
}
main();
"#,
    );
    assert_eq!(
        out.trim(),
        "map:2,4,6\nspawn:42\ntimers:before-worker,after-worker"
    );
}

/// `Atomics.waitAsync` resolves through the same pending-result queue as
/// `spawn`, but its promise is created by the *calling* agent while the futex
/// waiter thread (which owns no heap and runs no JS) queues the result. The
/// owner tag must be captured at the call site, or the result is filed under the
/// wrong agent and the promise never settles (the test hangs).
#[test]
fn atomics_wait_async_resolves_through_the_owner_tagged_queue() {
    let out = compile_and_run(
        r#"
import { spawn } from "perry/thread";
const sab = new SharedArrayBuffer(4);
const a = new Int32Array(sab);
const r = Atomics.waitAsync(a, 0, 0, 10000) as { async: boolean; value: Promise<string> };
console.log("async:" + r.async);
spawn(() => {
  const view = new Int32Array(sab);
  const nap = new Int32Array(new SharedArrayBuffer(4));
  Atomics.wait(nap, 0, 0, 30);
  return Atomics.notify(view, 0);
});
const result = await r.value;
console.log("result:" + result);
"#,
    );
    assert_eq!(out.trim(), "async:true\nresult:ok");
}
