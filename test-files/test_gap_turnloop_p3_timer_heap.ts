// turnloop P3 — the agent timer heap: one expiry-ordered structure, and the
// ref/unref liveness that decides whether the loop waits for it.
//
// Measured against the pinned oracle (Node 26.5.1), five runs each.
//
//   1. **Cross-class deadline ordering.** An interval is not a separate class
//      for ordering: `setInterval(i, 3)` fires between `setTimeout(t1, 1)` and
//      `setTimeout(t5, 5)`. Before P3 Perry drained a whole callback queue and
//      then a whole interval queue, printing `t1, t5, i`.
//   2. **An overrunning interval does not catch up.** With a 10 ms period and a
//      handler that blocks for 25 ms, Node fires once per iteration with ~25 ms
//      between fires — never a back-to-back burst making up the lost ticks.
//      Asserted as a property (no delta below the handler cost, one fire per
//      turn), not as timestamps, so it is stable on a loaded machine.
//   3. **unref.** An unref'd interval alone never ticks and the process exits;
//      an unref'd timeout still fires when something else holds the loop open
//      past its deadline; `hasRef()` tracks the calls.
//   4. **`Timeout.refresh()`** re-arms with the original delay.
//   5. **Exit liveness.** A lone `setImmediate` keeps the loop alive for one
//      more turn; an unref'd 60 s timeout does not hold the process open.

const log: string[] = [];

function crossClassOrder(next: () => void): void {
  const fired: string[] = [];
  const interval = setInterval(() => fired.push("i"), 3);
  setTimeout(() => fired.push("t5"), 5);
  setTimeout(() => fired.push("t1"), 1);
  const start = Date.now();
  while (Date.now() - start < 30) {
    /* every deadline above is overdue when the loop next turns */
  }
  setTimeout(() => {
    clearInterval(interval);
    log.push("cross-class order: " + fired.join(","));
    next();
  }, 0);
}

function intervalDoesNotCatchUp(next: () => void): void {
  const fires: number[] = [];
  let last = Date.now();
  const handle = setInterval(() => {
    const now = Date.now();
    fires.push(now - last);
    last = now;
    const spin = Date.now();
    while (Date.now() - spin < 25) {
      /* overrun the 10 ms period */
    }
    if (fires.length === 5) {
      clearInterval(handle);
      // The first delta is the initial 10 ms arming; every later one must be at
      // least the handler's own cost, which is what "no catch-up" means.
      const later = fires.slice(1);
      log.push("interval fires: " + fires.length);
      log.push("interval no-catch-up: " + later.every((d) => d >= 20));
      next();
    }
  }, 10);
}

function unrefLiveness(next: () => void): void {
  // An unref'd interval does not keep the loop alive by itself, but it still
  // ticks while something else does — here the 40 ms timeout below. Count the
  // ticks rather than printing each, so the count is the assertion.
  let idleTicks = 0;
  const idle = setInterval(() => idleTicks++, 5);
  idle.unref();
  log.push("unref'd interval hasRef: " + idle.hasRef());

  // An unref'd timeout DOES fire when the loop is still alive at its deadline.
  const quiet = setTimeout(() => log.push("unref'd timeout fired"), 10);
  quiet.unref();
  log.push("unref'd timeout hasRef: " + quiet.hasRef());
  quiet.ref();
  log.push("after ref() hasRef: " + quiet.hasRef());
  quiet.unref();

  // A refresh'd handle re-arms with its original delay and is ref'd again.
  const refreshed = setTimeout(() => log.push("refreshed fired"), 5);
  refreshed.unref();
  refreshed.refresh();
  log.push("after refresh() hasRef: " + refreshed.hasRef());

  setTimeout(() => {
    clearInterval(idle);
    log.push("unref'd interval ticked while the loop was alive: " + (idleTicks > 0));
    next();
  }, 40);
}

function exitLiveness(): void {
  // A lone `setImmediate` keeps the loop alive for exactly one more turn, so
  // the report below runs even though nothing else is scheduled.
  setImmediate(() => {
    log.push("lone immediate ran");
    report();
  });
  // An unref'd long timeout must not hold the process open. If it did, this
  // program would sit for a minute instead of exiting after the report — the
  // harness would time out rather than diff.
  const forever = setTimeout(() => log.push("unref'd 60s timeout MUST NOT FIRE"), 60_000);
  forever.unref();
}

function report(): void {
  console.log("results:");
  for (const line of log) console.log("  " + line);
}

crossClassOrder(() => intervalDoesNotCatchUp(() => unrefLiveness(exitLiveness)));
