// Behavioral parity test for AsyncLocalStorage (perry-stdlib).
//
// Pairs with test_parity_async_hooks.ts but focuses on store
// propagation semantics with deterministic assertions.

import { AsyncLocalStorage } from "node:async_hooks";

const als = new AsyncLocalStorage<{ id: number; tag: string }>();

// ── Baseline: getStore outside of run() is undefined ──
console.log("getStore outside run:", als.getStore());

// ── run(store, fn) — synchronous callback sees the store ──
const sync = als.run({ id: 1, tag: "sync" }, () => {
  const store = als.getStore();
  return store ? `${store.id}:${store.tag}` : "missing";
});
console.log("run sync result:", sync);

// ── Nested run — inner store shadows outer, restored on exit ──
als.run({ id: 1, tag: "outer" }, () => {
  console.log("outer tag:", als.getStore()!.tag);
  als.run({ id: 2, tag: "inner" }, () => {
    console.log("inner tag:", als.getStore()!.tag);
  });
  console.log("after inner, outer tag again:", als.getStore()!.tag);
});

// ── exit() — runs a callback outside of any store ──
als.run({ id: 1, tag: "running" }, () => {
  als.exit(() => {
    console.log("inside exit, getStore:", als.getStore());
  });
  console.log("after exit, tag again:", als.getStore()!.tag);
});

// ── disable() removes the active store for the active async context ──
als.run({ id: 1, tag: "before disable" }, () => {
  console.log("pre-disable tag:", als.getStore()!.tag);
  als.disable();
  console.log("post-disable getStore:", als.getStore());
});

/*
@covers
crates/perry-stdlib/src/async_local_storage.rs:
  - js_async_local_storage_disable
  - js_async_local_storage_enter_with
  - js_async_local_storage_exit
  - js_async_local_storage_get_store
  - js_async_local_storage_new
  - js_async_local_storage_run
*/
