// Issue #859 regression: native-bridged Promises (allocated via
// `js_promise_new` and shipped to a tokio worker through
// `spawn_for_promise` / `perry_ffi_promise_new`) must stay GC-rooted
// while the worker is running.
//
// Pre-fix, the Promise had no live root once the awaiter yielded:
// `P.next = N` is a forward edge, so after the user code suspended
// on `await argon2.hash(pw)` all reachable JS roots pointed at `N`,
// never back at `P`. The tokio future captured `promise_ptr: usize`,
// invisible to the GC. A GC fired during the worker's run swept `P`,
// and the later `js_promise_resolve(*mut Promise, value)` dereferenced
// freed memory — SIGBUS on macOS once the arena's idle-block reclaim
// (Phase C4b-δ) had handed the page back to the OS.
//
// The fix pins the Promise in `spawn_for_promise[_deferred]` and in
// `perry_ffi_promise_new` (the entry points that all stdlib + perry-ext
// native bindings funnel through). Unpin happens in
// `js_stdlib_process_pending` immediately before resolve/reject.
//
// This test exercises the shape — repeated `await argon2.hash()` with
// heavy allocation between awaits to force GC cycles while the next
// argon2 worker is still computing. Pre-fix, this either SIGBUSed or
// silently corrupted the hash string (depending on which page the
// arena reused). Post-fix, every iteration returns a well-formed
// $argon2id$ hash.

import argon2 from "argon2";

async function inner(pw: string): Promise<string> {
  const h = await argon2.hash(pw);
  return h;
}

function pressure(): string[] {
  // Push enough allocation through the nursery to force at least one
  // GC cycle per iteration on the default 64 MB initial threshold.
  const out: string[] = [];
  for (let i = 0; i < 30000; i++) {
    out.push("pressure-block-" + i + "-xxxxxxxxxxxxxxxxxxxxxx");
  }
  return out;
}

async function handler(i: number): Promise<number> {
  const a = await inner("pw-a-" + i);
  // Sanity check: hash must be a valid argon2id string. Pre-fix, GC
  // could swap the string out from under us if the hash pointer landed
  // in a reclaimed block.
  if (!a.startsWith("$argon2id$")) {
    throw new Error("iter " + i + " corrupted hash a: " + a);
  }
  const big1 = pressure();
  const b = await inner("pw-b-" + i);
  if (!b.startsWith("$argon2id$")) {
    throw new Error("iter " + i + " corrupted hash b: " + b);
  }
  const big2 = pressure();
  const c = await inner("pw-c-" + i);
  if (!c.startsWith("$argon2id$")) {
    throw new Error("iter " + i + " corrupted hash c: " + c);
  }
  return a.length + b.length + c.length + big1.length + big2.length;
}

async function main() {
  let total = 0;
  for (let i = 0; i < 20; i++) {
    total += await handler(i);
  }
  console.log("iterations completed without crash or hash corruption");
}

await main();
console.log("issue 859 regression: ok");
