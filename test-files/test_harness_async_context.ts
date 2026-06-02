// #807 — real lifecycle harness for async_hooks + AsyncLocalStorage
// across `await`, microtasks, and timers.
//
// Today these APIs are name-only stubs in perry-stdlib (#788, #789):
// the constructors exist but `getStore()` doesn't actually track the
// active async context through await boundaries. This harness gives
// us a deterministic, byte-for-byte parity probe so we can watch the
// gap close as #788/#789 land — and catch regressions afterwards.
//
// Each section prints `section: result` so a parity diff pins the
// exact propagation rule that broke.

import { AsyncLocalStorage, createHook, executionAsyncId } from "node:async_hooks";

// ── 1. Synchronous store propagation (baseline) ────────────────────────────
// If this fails, nothing else can work — covers the AsyncLocalStorage
// shape itself, not propagation.
const als = new AsyncLocalStorage<{ trace: string }>();
const syncResult = als.run({ trace: "sync" }, () => als.getStore()?.trace);
console.log("sync:", syncResult);

// ── 2. Propagation through a single await ──────────────────────────────────
// `await` suspends and resumes the function inside a microtask; the
// store must follow the continuation. This is the #788 acceptance.
async function awaitedTrace(): Promise<string | undefined> {
  await Promise.resolve();
  return als.getStore()?.trace;
}
async function section2(): Promise<void> {
  const out = await als.run({ trace: "await" }, awaitedTrace);
  console.log("await:", out);
}

// ── 3. Propagation through chained awaits ──────────────────────────────────
// Each await is its own microtask; the chain must keep the store alive
// across all of them (no decay between the 1st and Nth).
async function chainedTrace(depth: number): Promise<string | undefined> {
  for (let i = 0; i < depth; i++) {
    await Promise.resolve();
  }
  return als.getStore()?.trace;
}
async function section3(): Promise<void> {
  const out = await als.run({ trace: "chain" }, () => chainedTrace(8));
  console.log("chain8:", out);
}

// ── 4. Propagation through queueMicrotask ──────────────────────────────────
// Microtasks scheduled inside a run() must see the store. Node's
// AsyncLocalStorage tracks this through the async-resource bridge.
function microtaskTrace(): Promise<string | undefined> {
  return new Promise((resolve) => {
    queueMicrotask(() => resolve(als.getStore()?.trace));
  });
}
async function section4(): Promise<void> {
  const out = await als.run({ trace: "microtask" }, microtaskTrace);
  console.log("microtask:", out);
}

// ── 5. Propagation through setTimeout(fn, 0) ───────────────────────────────
// Timers register an async resource and resume into a new tick; the
// store still has to follow.
function timerTrace(): Promise<string | undefined> {
  return new Promise((resolve) => {
    setTimeout(() => resolve(als.getStore()?.trace), 0);
  });
}
async function section5(): Promise<void> {
  const out = await als.run({ trace: "timer" }, timerTrace);
  console.log("timer:", out);
}

// ── 6. Nested run() with await in between ──────────────────────────────────
// Outer store survives, inner shadows during its run, outer restored on
// inner's exit — across an await boundary. Catches the "store stack
// flattens on suspension" failure mode.
async function section6(): Promise<void> {
  await als.run({ trace: "outer6" }, async () => {
    console.log("outer6 pre:", als.getStore()?.trace);
    await als.run({ trace: "inner6" }, async () => {
      await Promise.resolve();
      console.log("inner6:", als.getStore()?.trace);
    });
    await Promise.resolve();
    console.log("outer6 post:", als.getStore()?.trace);
  });
}

// ── 7. Concurrent runs interleaved by Promise.all ──────────────────────────
// Two awaited callbacks scheduled into the same tick must each see
// their own store, not cross-pollinate. Node guarantees per-callback
// isolation; this is where naive "single global active store"
// implementations leak.
async function section7(): Promise<void> {
  const fA = als.run({ trace: "A" }, async () => {
    await Promise.resolve();
    return als.getStore()?.trace;
  });
  const fB = als.run({ trace: "B" }, async () => {
    await Promise.resolve();
    return als.getStore()?.trace;
  });
  const results = await Promise.all([fA, fB]);
  console.log("concurrent A:", results[0]);
  console.log("concurrent B:", results[1]);
}

// ── 8. async_hooks.createHook init/before/after/destroy lifecycle ──────────
// Hook callbacks must fire on real async resources. We don't assert
// counts (Node and Perry will differ on hidden internals); we assert
// shape:
//   - createHook returns an object with the four methods.
//   - hook.enable() and hook.disable() are no-ops we can call safely.
//   - executionAsyncId() returns a positive integer.
// The whole point of #789 is that all four callbacks fire; today this
// section just exercises the API surface.
function section8(): void {
  const hook = createHook({
    init() {},
    before() {},
    after() {},
    destroy() {},
  });
  console.log("createHook typeof:", typeof hook);
  console.log("enable typeof:", typeof hook.enable);
  console.log("disable typeof:", typeof hook.disable);
  hook.enable();
  hook.disable();
  const id = executionAsyncId();
  console.log("executionAsyncId number:", typeof id === "number");
  console.log("executionAsyncId positive:", id >= 0);
}

// ── Driver: run sections sequentially so output is deterministic ──────────
(async () => {
  await section2();
  await section3();
  await section4();
  await section5();
  await section6();
  await section7();
  section8();
  console.log("harness: done");
})().catch((e: unknown) => {
  console.log("harness: ERROR", (e as Error).message);
});
