// Regression test for #1663: SIGSEGV in js_promise_run_microtasks during
// async resumption — the deeper, Fastify-handler-shaped variant.
//
// This widens the original test_issue_1663_async_reentry_microtask.ts case:
// a prior COMPLETED awaiting-call (like a Fastify onRequest hook), followed
// by a non-transformed async closure (like a route handler passed to
// app.post) that performs THREE sequential awaiting-calls (the read-body →
// SELECT → INSERT chain). Each `await` re-entrantly drains the microtask
// queue; before #1675 the Task::Promise arm reloaded the running promise
// from a clobbered TLS cell and dereferenced `(*promise).async_id` (offset
// 0x30) on a NULL pointer. This shape exercises a deeper re-entrant nesting
// than the original repro.
//
// Expected output (byte-identical to `node --experimental-strip-types`):
//   prior: ok
//   handler: 6
//   reached

async function awaitingCall(n: number): Promise<number> {
  await Promise.resolve();
  return n;
}

async function runCb(cb: () => Promise<void>) {
  await cb();
}

async function priorCompleted() {
  await Promise.resolve();
  console.log("prior: ok");
}

async function handler() {
  await runCb(async () => {
    const a = await awaitingCall(1);
    const b = await awaitingCall(2);
    const c = await awaitingCall(3);
    console.log("handler:", a + b + c);
  });
}

(async () => {
  await runCb(priorCompleted);
  await handler();
  console.log("reached");
})();
