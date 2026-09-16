// turnloop P9: what can a `perry/thread` agent actually reach?
//
// The brief's acceptance case names two kinds of non-primary agent. The
// `worker_threads` Worker is covered by `p9_worker_agent_acceptance.ts`. This
// is the other one, and it exists to establish a fact rather than to assert a
// pass: `perry/thread`'s `spawn` / `parallelMap` / `parallelFilter` call the
// user closure ONCE, synchronously, and serialize its return value
// (`crates/perry-runtime/src/thread.rs`). There is no microtask pump, no timer
// tick and no `js_wait_for_event` anywhere in those thread bodies, so such an
// agent has no event loop to give a loop to.
//
// That is a claim about Rust. This makes it a measurement: the same async
// work, on the primary agent and inside each of the three `perry/thread`
// entry points. If a `perry/thread` agent could await, the second and third
// rows would carry a value; if it cannot, they say what they returned instead.
// Either way the run distinguishes "P9 did not reach this surface" from "this
// surface has nothing to reach" — which a green suite cannot.
import { parallelMap, spawn } from "perry/thread";

const url = process.env.P9_URL ?? "http://127.0.0.1:8099/";

async function primary(): Promise<string> {
  try {
    const r = await fetch(url);
    return `status=${r.status} bytes=${(await r.text()).length}`;
  } catch (e) {
    return "error:" + (e as Error).message;
  }
}

console.log(`primary-agent fetch: ${await primary()}`);

// `spawn`: the closure runs on its own agent and its RETURN VALUE crosses back
// by deep copy. An async closure returns a Promise, which is not a value that
// can cross an agent boundary, so what arrives here is the interesting part.
try {
  const spawned = await spawn(() => {
    // Deliberately synchronous: this is what the surface supports today.
    let n = 0;
    for (let i = 0; i < 1000; i++) n += i;
    return `sync-ok sum=${n}`;
  });
  console.log(`thread-agent spawn(sync): ${spawned}`);
} catch (e) {
  console.log(`thread-agent spawn(sync): error:${(e as Error).message}`);
}

try {
  const spawnedAsync = await spawn(async () => {
    const r = await fetch(url);
    return `status=${r.status}`;
  });
  console.log(`thread-agent spawn(async): ${JSON.stringify(spawnedAsync)}`);
} catch (e) {
  console.log(`thread-agent spawn(async): error:${(e as Error).message}`);
}

try {
  const mapped = parallelMap([1, 2, 3, 4], (x: number) => x * x);
  console.log(`thread-agent parallelMap(sync): ${JSON.stringify(mapped)}`);
} catch (e) {
  console.log(`thread-agent parallelMap(sync): error:${(e as Error).message}`);
}

console.log("done");
