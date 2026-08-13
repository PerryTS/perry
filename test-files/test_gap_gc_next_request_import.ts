// #8036: preserve a production-shaped request object when a generated route
// module re-exports its imported user handler. The request and all of its
// nested state must remain live while the callee allocates, dynamically
// imports a module, resumes a promise, resumes a timer, and reads the body.
// The GC moving-witness matrix registers this test below and runs it with
// forced/verified evacuation so a stale-but-unreused pointer cannot pass.

import { GET, POST, syncSummary } from "./fixtures/gc_next_request_import/route.ts";
import { FixtureNextRequest } from "./fixtures/gc_next_request_import/request.ts";

function makeRequest(id: string, iterations: number, method = "GET", body = "") {
  return new FixtureNextRequest(
    method,
    "https://perry.invalid/api/benchmark?id=" + id + "&iterations=" + iterations,
    id,
    body,
  );
}

const syncRequest = makeRequest("sync-request", 17);
console.log("sync", syncSummary(syncRequest));

// Mirror the production verifier's 20 concurrent GETs without making this
// request-boundary regression depend on Promise.all's separate array-forwarding
// contract. Starting every handler before awaiting the first result also makes
// cross-request ID/header swaps observable.
const pending: Array<Promise<Record<string, unknown>>> = [];
for (let index = 0; index < 20; index += 1) {
  pending.push(GET(makeRequest("request-" + index, index + 1)));
}

// Perry exposes an explicit collector hook; Node does not unless launched with
// --expose-gc. Under the matrix's force/verify arm this evacuates the request
// graphs while every handler is suspended and all 20 timers are still queued.
const collect = (globalThis as unknown as { gc?: () => void }).gc;
if (collect) {
  collect();
}

for (const result of pending) {
  console.log(JSON.stringify(await result));
}

console.log(
  JSON.stringify(
    await POST(makeRequest("post-request", 31, "POST", "perry-request-body")),
  ),
);
