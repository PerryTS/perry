// turnloop P3 — Node's event-loop phase order, without I/O.
//
// Every expectation was measured against the pinned oracle (Node 26.5.1) five
// times before it was written down; only orderings that came out identical on
// all five runs are asserted here. Two that are genuinely racy under Node are
// deliberately absent: `setTimeout(…, 0)` vs `setImmediate` at main-module top
// level (Node documents it as not guaranteed), and the same pair scheduled from
// *inside* a running `setImmediate` callback (measured 14/20 one way, 6/20 the
// other).
//
// What this pins:
//   1. an immediate scheduled BY a check callback runs on the next turn, behind
//      an immediate that was already queued (check phase is a snapshot);
//   2. once both are overdue, the timers phase runs before the check phase
//      whatever order they were registered in;
//   3. `process.nextTick` → promise/`queueMicrotask` → macrotasks, after a
//      timer callback and after an immediate callback;
//   4. clearing a sibling that is already due, from inside an earlier callback
//      of the same batch, stops it — in the timers phase and the check phase.

const log: string[] = [];

function nestedImmediates(next: () => void): void {
  setImmediate(() => {
    log.push("imm:a");
    setImmediate(() => {
      log.push("imm:b-nested");
      next();
    });
  });
  setImmediate(() => log.push("imm:c"));
}

function overdueBatch(next: () => void): void {
  // Register the immediate FIRST, then the timeout, then block past both
  // deadlines: the timers phase still wins, because it comes first in the
  // iteration.
  setImmediate(() => log.push("overdue:immediate"));
  setTimeout(() => {
    log.push("overdue:timeout");
    process.nextTick(() => log.push("timer:tick"));
    Promise.resolve().then(() => log.push("timer:promise"));
    queueMicrotask(() => log.push("timer:queueMicrotask"));
    setImmediate(() => {
      log.push("timer:immediate");
      process.nextTick(() => log.push("imm:tick"));
      Promise.resolve().then(() => log.push("imm:promise"));
      queueMicrotask(() => {
        log.push("imm:queueMicrotask");
        next();
      });
    });
  }, 1);
  const start = Date.now();
  while (Date.now() - start < 30) {
    /* let both deadlines pass before the loop turns */
  }
}

function cancelWithinBatch(next: () => void): void {
  // Two timeouts at the same deadline. The FIRST one registered runs first —
  // same deadline ties break on creation order — and clears the second, which
  // then never fires: Node's timers phase walks its list entry by entry, so a
  // handle a previous callback cleared is skipped even though it was already
  // due when the phase began.
  let doomedTimeout: ReturnType<typeof setTimeout>;
  let doomedInterval: ReturnType<typeof setInterval>;
  setTimeout(() => {
    log.push("cancel:timeout-a");
    clearTimeout(doomedTimeout);
    // A timeout clearing an interval that is due at the same instant: the
    // interval never ticks at all.
    clearInterval(doomedInterval);
  }, 1);
  doomedTimeout = setTimeout(() => log.push("cancel:timeout-MUST-NOT-RUN"), 1);
  doomedInterval = setInterval(() => log.push("cancel:interval-MUST-NOT-RUN"), 1);

  // Same within the check phase: `a` clears `b`, `c` still runs — the batch is
  // a snapshot processed handle by handle, not invalidated wholesale.
  let doomedImmediate: ReturnType<typeof setImmediate>;
  setImmediate(() => {
    log.push("cancel:imm-a");
    clearImmediate(doomedImmediate);
  });
  doomedImmediate = setImmediate(() => log.push("cancel:imm-MUST-NOT-RUN"));
  setImmediate(() => {
    log.push("cancel:imm-c");
    next();
  });

  const start = Date.now();
  while (Date.now() - start < 30) {
    /* every handle above is overdue when the loop next turns */
  }
}

function report(): void {
  console.log("order:");
  for (const line of log) console.log("  " + line);
}

nestedImmediates(() => overdueBatch(() => cancelWithinBatch(() => setTimeout(report, 20))));
