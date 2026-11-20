// #9441: a program that has finished its work must not linger.
//
// `IDLE_CAP_MS = 1000` in crates/perry-runtime/src/event_pump.rs is the wait
// budget `js_wait_for_event` uses when no timer deadline is nearer. The
// generated event loop parks on it at the END of every body iteration —
// INCLUDING the iteration that consumed the last event source. Nothing can wake
// that park, because the state it is waiting for a change in has already
// changed: the answer is known and the pump is asleep on it. The next loop
// header therefore discovers "nothing left to do" a full second late, and that
// second is pure user-visible latency on every short-lived program.
//
// This fixture measures the tail rather than asserting an absolute duration,
// because the machine it runs on is shared: each role's wall clock is compared
// against a `baseline` role that exits in its first tick, so process spawn,
// dynamic linking and runtime init cancel out and only the idle tail is left.
// Three samples are taken of each and the MINIMUM is compared, so a scheduling
// spike inflates a sample without changing the verdict. The 400 ms threshold
// sits between the ~1000 ms structural tail and the ~20 ms a healthy exit costs
// — wide enough that it is not a benchmark, narrow enough that it cannot pass
// with the park still in place.
//
// Measured on the reporting machine, `origin/main` @1659211e7c, min of 5 runs
// under load ~105: baseline 45 ms, timer-idle 1082 ms, stdin-end likewise past
// the budget, promise-idle 38 ms. Node: baseline 110 ms, timer-idle 132 ms.
//
// `refed-timer` is the anti-regression arm: a program with a live 300 ms timer
// must NOT exit early. A fix that simply stopped parking, or that widened the
// "nothing is live" test until it swallowed a pending timer, reports here — and
// test_gap_9416_stdin_only_loop_liveness covers the same ground for stdin.
import { spawn } from "node:child_process";

const ROLE_ENV = "PERRY_9441_ROLE";
const SAMPLES = 3;
// The tail is ~1000 ms structurally and ~20 ms once the park is skipped.
const TAIL_BUDGET_MS = 400;
// `refed-timer` holds a 300 ms timer; allow generous slack under load.
const REFED_MIN_MS = 200;
const WATCHDOG_MS = 8000;

const role = process.env[ROLE_ENV] ?? "";

if (role === "baseline") {
  // Exits in its first tick: the cost of spawn + init and nothing else.
  process.exit(0);
} else if (role === "timer-idle") {
  // One short timer, then the loop is empty. The park that follows the firing
  // iteration is the bug.
  setTimeout(() => {}, 20);
} else if (role === "promise-idle") {
  // CONTROL, not a witness: this shape was already fast before the fix
  // (measured 38 ms against the timer arm's 1082 ms), because the pump's
  // NOTIFIED fast path returns without ever computing a budget when a promise
  // resolution flipped the flag. It is here so a fix that reorganises the wait
  // cannot make the ALREADY-fast path slow while making the slow one fast.
  void Promise.resolve().then(() => {});
} else if (role === "stdin-end") {
  // The reported probe: a stdin reader whose pipe is closed immediately. The
  // last source goes away when `'end'` is dispatched, on the main thread, in
  // the same iteration that then parks.
  const stream: any = process.stdin;
  stream.once("end", () => {});
  stream.resume();
} else if (role === "refed-timer") {
  // ANTI-REGRESSION: a live 300 ms timer must still hold the loop open.
  setTimeout(() => {}, 300);
} else {
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const runOnce = (name: string, closeStdin: boolean) =>
    new Promise<number>((resolve) => {
      const started = Date.now();
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name },
        stdio: [closeStdin ? "pipe" : "ignore", "inherit", "inherit"],
      });
      let settled = false;
      const watchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        child.kill("SIGKILL");
        resolve(WATCHDOG_MS);
      }, WATCHDOG_MS);
      child.on("exit", () => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        resolve(Date.now() - started);
      });
      if (closeStdin) {
        child.stdin!.end();
      }
    });

  const best = async (name: string, closeStdin = false) => {
    let min = Number.POSITIVE_INFINITY;
    for (let i = 0; i < SAMPLES; i++) {
      const ms = await runOnce(name, closeStdin);
      if (ms < min) min = ms;
    }
    return min;
  };

  (async () => {
    const baseline = await best("baseline");
    const timerIdle = await best("timer-idle");
    const promiseIdle = await best("promise-idle");
    const stdinEnd = await best("stdin-end", true);
    const refed = await best("refed-timer");

    console.log("timer-idle tail under budget:", timerIdle - baseline < TAIL_BUDGET_MS);
    console.log("promise-idle tail under budget:", promiseIdle - baseline < TAIL_BUDGET_MS);
    console.log("stdin-end tail under budget:", stdinEnd - baseline < TAIL_BUDGET_MS);
    console.log("refed-timer held the loop open:", refed - baseline >= REFED_MIN_MS);
    console.log("done");
  })();
}
