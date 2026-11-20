// #9305 regression: a JS throw inside a microtask longjmp-lands in the
// microtask runner's trap. Pre-fix, the runner armed `setjmp` directly from
// Rust; rustc cannot express `returns_twice`, so LLVM colored the stack slot
// holding the spilled TLS-base temporary into the task-record copy loop that
// runs on every popped task — the landing then reloaded a clobbered slot and
// crashed (NULL TLS base). The scenario needs (a) popped Task::Promise
// records ahead of the throw and (b) a throw that reaches the runner's trap,
// i.e. a `.then` callback throwing with no try/catch of its own.
//
// Expected output is byte-identical to `node` (golden captured from
// node; microtask FIFO order is deterministic).
const log: string[] = [];

// Benign microtasks first: each popped task runs the record-copy loop that
// reused the trap's colored slot pre-fix.
Promise.resolve(1)
  .then((v) => v + 1)
  .then((v) => {
    log.push("benign:" + v);
  });

// Throw from a .then callback — reaches the runner's trap, which must
// reject the chained promise.
Promise.resolve("x")
  .then(() => {
    throw new Error("boom-9305");
  })
  .catch((e: Error) => {
    log.push("caught:" + e.message);
  });

// Rethrow through a chain: two landings in one drain family.
Promise.reject(new Error("first"))
  .catch((e: Error) => {
    throw new Error("re:" + e.message);
  })
  .catch((e: Error) => {
    log.push("recaught:" + e.message);
  });

// Throw inside a local try inside a microtask: the generated landing pad
// catches it; the runner's trap stays armed and undisturbed.
Promise.resolve().then(() => {
  try {
    throw new Error("inner");
  } catch (e) {
    log.push("inner-caught:" + (e as Error).message);
  }
});

// queueMicrotask callback that throws AFTER a caught landing in the same
// drain — exercises the trap re-arm path; its rejection routing goes
// through the queued-microtask context restore.
Promise.resolve()
  .then(() => {
    throw new Error("second-landing");
  })
  .catch((e: Error) => {
    log.push("second:" + e.message);
  });

setTimeout(() => {
  console.log(log.join("\n"));
}, 0);
console.log("sync-done");
