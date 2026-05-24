// Regression test for #1663: SIGSEGV in js_promise_run_microtasks during
// async resumption.
//
// A non-transformed async closure's `await` busy-waits by re-entrantly
// draining the microtask queue. Each nested `Task::Promise` dispatch
// overwrote (and on exit nulled) the CURRENT_MICROTASK_PROMISE / _NEXT
// thread-locals; when the enclosing dispatch resumed it reloaded `promise`
// from the now-null cell and dereferenced `(*promise).async_id` (offset
// 0x30) — a deterministic segfault. The trigger needs a prior completed
// awaiting-call (`viaAwait`) followed by a subsequently-awaited nested
// awaiting-call (`nested`). GC is NOT required.
//
// Expected output (byte-identical to `node --experimental-strip-types`):
//   await: ok
//   outer post: ok
//   reached

async function runCb(cb: () => Promise<void>) {
  await cb();
}

async function viaAwait() {
  await Promise.resolve();
  console.log("await: ok");
}

async function nested() {
  await runCb(async () => {
    await runCb(async () => {
      await Promise.resolve();
    });
    await Promise.resolve();
    console.log("outer post: ok");
  });
}

(async () => {
  await runCb(viaAwait);
  await nested();
  console.log("reached");
})();
