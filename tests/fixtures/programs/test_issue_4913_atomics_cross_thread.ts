// #4913 Stage 2 — real cross-agent Atomics: a SharedArrayBuffer aliases the
// same physical bytes across a `perry/thread` boundary, and Atomics.wait /
// notify / waitAsync block and wake for real (no more "timed-out" lie).
import { spawn } from "perry/thread";

// ── 1. Real blocking: wait on a matching value with no notifier must actually
//      block for ~the timeout, then return "timed-out" (the old stub returned
//      immediately). ──────────────────────────────────────────────────────────
{
  const sab = new SharedArrayBuffer(4);
  const a = new Int32Array(sab);
  const t0 = Date.now();
  const r = Atomics.wait(a, 0, 0, 120);
  const elapsed = Date.now() - t0;
  console.log("1-result", r);
  console.log("1-blocked", elapsed >= 100);
}

// ── 2. Cross-thread shared memory + notify wakes a parked waiter. The worker
//      sleeps ~100ms (Atomics.wait timeout on its own buffer) so the main
//      thread is parked first, then stores 42 and notifies. ───────────────────
{
  const sab = new SharedArrayBuffer(8);
  const main = new Int32Array(sab);
  const worker = spawn(() => {
    const view = new Int32Array(sab); // captures `sab` → aliases the same bytes
    const nap = new Int32Array(new SharedArrayBuffer(4));
    Atomics.wait(nap, 0, 0, 100); // sleep ~100ms so main parks first
    Atomics.store(view, 0, 42);
    return Atomics.notify(view, 0);
  });
  const status = Atomics.wait(main, 0, 0, 10000);
  const value = Atomics.load(main, 0);
  const woke = await worker;
  console.log("2-status", status);
  console.log("2-value", value);
  console.log("2-woke", woke);
}

// ── 3. Atomics.waitAsync resolves on a cross-thread notify. The async waiter is
//      enqueued synchronously here, BEFORE the worker spawns, so the notify can
//      never be missed → deterministic "ok". ──────────────────────────────────
{
  const sab = new SharedArrayBuffer(4);
  const a = new Int32Array(sab);
  const r = Atomics.waitAsync(a, 0, 0, 10000) as { async: boolean; value: Promise<string> };
  console.log("3-async", r.async);
  spawn(() => {
    const view = new Int32Array(sab);
    const nap = new Int32Array(new SharedArrayBuffer(4));
    Atomics.wait(nap, 0, 0, 30); // brief nap so the main loop reaches `await`
    return Atomics.notify(view, 0);
  });
  const result = await r.value;
  console.log("3-result", result);
}
