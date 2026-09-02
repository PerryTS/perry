// #9552: a promise minted for a cross-thread settlement — every stdlib
// `fetch` response, among ~110 stdlib call sites — is referenced only by the
// worker's raw address while the request is in flight: the awaiting
// continuation hangs OFF the promise (`P.on_fulfilled`), nothing on the JS
// side points AT it. A full collection landing in that window freed it, and
// the completion then resolved whatever the allocator had reused the slot for
// (the report: a RegExp header read as a promise's `next`, SIGSEGV in the
// microtask pump).
//
// The constructor now pins the promise until it settles. This runs a request
// against a local server that answers late, forces collections while it is in
// flight, and expects the response to arrive. Node only exposes `gc` under
// --expose-gc, so the collection is conditional and the expected output is
// identical on both.
import http from "node:http";

declare const gc: undefined | (() => void);

const server = http.createServer((_req, res) => {
  setTimeout(() => {
    res.end("ok");
  }, 60);
});

async function get(url: string): Promise<string> {
  const response = await fetch(url);
  return await response.text();
}

server.listen(0, "127.0.0.1", async () => {
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  const inflight = get(`http://127.0.0.1:${port}/`);

  let junk: Array<{ k: number; s: string }> = [];
  for (let round = 0; round < 8; round++) {
    junk = Array.from({ length: 4000 }, (_, k) => ({ k, s: "x".repeat(40) }));
    if (typeof gc === "function") gc();
    await new Promise((resolve) => setTimeout(resolve, 5));
  }

  console.log(await inflight, junk.length);
  server.close();
});
