// End-to-end coverage for fetch's three RequestInit.redirect modes
// ('follow', 'manual', 'error') (#11136). The mode plumbing itself is unit
// tested (the FetchRedirectMode -> turnloop_http::client::RedirectMode
// mapping, the HIR lowering); nothing else exercises the engine's actual
// follow/expose-as-is/reject behavior end to end, the rejection shape, or
// the 'redirected' / 'type' / final-url response metadata that go with it.
//
// Two local `node:http` servers on ephemeral ports stand in for two distinct
// origins, so a redirect between them is a genuine cross-origin hop (the
// `Authorization` header must be stripped). Ports are never printed --
// only normalized origin labels -- and nothing here depends on wall-clock
// time, so the output is deterministic across runs and across the Perry /
// Node oracle comparison.
//
// Measured against Node 26.5.1 (see the fix commit for the full transcript):
// - `redirect: 'manual'` on a 3xx does NOT produce a WHATWG "opaqueredirect"
//   filtered response (status 0, no headers) the way a browser's CORS layer
//   would. Node's fetch exposes the raw 3xx response as-is: real status,
//   real headers (including Location), type "basic", `redirected: false`,
//   and `url` equal to the REQUESTED url, not the Location target.
// - `redirect: 'error'` hitting a 3xx rejects with `TypeError: fetch failed`
//   whose cause is a bare `Error("unexpected redirect")` -- no `.code`.
// - Exceeding the redirect limit (20, matching turnloop-http's
//   DEFAULT_MAX_REDIRECTS) rejects the same way, cause
//   `Error("redirect count exceeded")`, also no `.code`.
// - `type` is "cors" once any followed hop leaves the request's first
//   origin, and stays "cors" even if a later hop comes back (A -> B -> A).
//   A direct request to another origin, with no redirect, is "basic".

import { createServer } from "node:http";
import type { IncomingMessage, ServerResponse } from "node:http";

function j(value: unknown): string {
  return JSON.stringify(value);
}

function listen(server: ReturnType<typeof createServer>): Promise<string> {
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      resolve(`http://127.0.0.1:${port}`);
    });
  });
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve) => {
    let body = "";
    req.on("data", (chunk: Buffer) => {
      body += chunk.toString("utf8");
    });
    req.on("end", () => resolve(body));
  });
}

async function main(): Promise<void> {
  let originA = "";
  let originB = "";
  let loopHits = 0;

  // Origin B: only ever reached via the cross-origin hop from origin A.
  const serverB = createServer((req: IncomingMessage, res: ServerResponse) => {
    if (req.url === "/landed") {
      const auth = req.headers["authorization"] ?? null;
      res.writeHead(200, { "content-type": "text/plain" });
      res.end("landed auth=" + j(auth));
      return;
    }
    if (req.url === "/back") {
      res.writeHead(302, { Location: originA + "/final", "content-length": "0" });
      res.end();
      return;
    }
    res.writeHead(404, { "content-length": "0" });
    res.end();
  });

  // Origin A: every redirect shape under test.
  const serverA = createServer((req: IncomingMessage, res: ServerResponse) => {
    const url = req.url ?? "";
    const redirectTo = (status: number, location: string) => {
      res.writeHead(status, { Location: location, "content-length": "0" });
      res.end();
    };

    if (url === "/loop") {
      loopHits++;
      redirectTo(302, "/loop");
      return;
    }
    if (url === "/r301") return redirectTo(301, "/final");
    if (url === "/r302") return redirectTo(302, "/final");
    if (url === "/r303") return redirectTo(303, "/final");
    if (url === "/r307") return redirectTo(307, "/final");
    if (url === "/r308") return redirectTo(308, "/final");
    // A mixed chain: 302 (rewriting) -> 307 (preserving) -> 303 (rewriting,
    // already-GET no-op by the time it's reached).
    if (url === "/chain1") return redirectTo(302, "/chain2");
    if (url === "/chain2") return redirectTo(307, "/chain3");
    if (url === "/chain3") return redirectTo(303, "/final");
    if (url === "/crosshop") return redirectTo(302, originB + "/landed");
    if (url === "/crossback") return redirectTo(302, originB + "/back");
    if (url === "/final") {
      readBody(req).then((body) => {
        const auth = req.headers["authorization"] ?? null;
        res.writeHead(200, { "content-type": "text/plain" });
        res.end("method=" + req.method + " body=" + j(body) + " auth=" + j(auth));
      });
      return;
    }
    res.writeHead(404, { "content-length": "0" });
    res.end();
  });

  originB = await listen(serverB);
  originA = await listen(serverA);

  function normalize(url: string): string {
    return url.replace(originA, "http://origin-a").replace(originB, "http://origin-b");
  }

  async function reportResponse(label: string, res: Response): Promise<void> {
    const body = await res.text().catch((e: any) => "TEXT_ERROR:" + e?.message);
    console.log(
      label +
        " status=" + res.status +
        " redirected=" + res.redirected +
        " type=" + res.type +
        " ok=" + res.ok +
        " url=" + normalize(res.url) +
        " body=" + j(body)
    );
  }

  async function reportManual(label: string, res: Response): Promise<void> {
    await reportResponse(label, res);
    console.log(
      label +
        " location=" + j(res.headers.get("location")) +
        " content-length=" + j(res.headers.get("content-length")) +
        " bodyUsed=" + res.bodyUsed
    );
  }

  async function reportThrow(label: string, go: () => Promise<Response>): Promise<void> {
    try {
      const res = await go();
      console.log(label + " NO_THROW status=" + res.status);
    } catch (error: any) {
      const cause = error?.cause;
      console.log(
        label +
          " threw " + error?.name + " " + j(error?.message) +
          " cause=" + (cause ? cause.name + " " + j(cause.message) : "none") +
          " causeCode=" + j(cause?.code ?? null)
      );
    }
  }

  // --- follow mode (default): every status code, simple GET ---
  for (const code of [301, 302, 303, 307, 308]) {
    const res = await fetch(originA + "/r" + code);
    await reportResponse("follow GET " + code, res);
  }

  // --- follow mode: POST + body, contrasting 301/302/303 (method rewritten
  // to GET, body cleared) against 307/308 (method and body preserved) ---
  for (const code of [301, 302, 303, 307, 308]) {
    const res = await fetch(originA + "/r" + code, {
      method: "POST",
      body: "payload-" + code,
      headers: { Authorization: "Bearer same-origin-token" },
    });
    await reportResponse("follow POST " + code, res);
  }

  // --- follow mode: a mixed multi-hop chain (302 -> 307 -> 303 -> 200) ---
  {
    const res = await fetch(originA + "/chain1", {
      method: "POST",
      body: "chain-body",
    });
    await reportResponse("follow chain", res);
  }

  // --- follow mode: explicit redirect: 'follow' is identical to the default ---
  {
    const res = await fetch(originA + "/r302", { redirect: "follow" });
    await reportResponse("follow explicit r302", res);
  }

  // --- follow mode: cross-origin hop strips Authorization ---
  {
    const res = await fetch(originA + "/crosshop", {
      headers: { Authorization: "Bearer cross-origin-token" },
    });
    await reportResponse("follow crosshop", res);
  }

  // --- follow mode: A -> B -> A; the response stays tainted "cors" ---
  {
    const res = await fetch(originA + "/crossback");
    await reportResponse("follow crossback", res);
  }

  // --- a direct request to the second origin, no redirect: "basic" ---
  {
    const res = await fetch(originB + "/landed");
    await reportResponse("direct origin-b", res);
  }

  // --- follow mode: too many redirects (loop exceeding the follow limit) ---
  await reportThrow("follow loop", () => fetch(originA + "/loop"));
  console.log("follow loop hits=" + loopHits);

  // --- manual mode: exposes the 3xx response as-is, does not follow ---
  {
    const res = await fetch(originA + "/r301", { redirect: "manual" });
    await reportManual("manual r301", res);
  }
  {
    const res = await fetch(originA + "/r302", { redirect: "manual" });
    await reportManual("manual r302", res);
  }
  {
    const res = await fetch(originA + "/r307", {
      method: "POST",
      body: "manual-body",
      redirect: "manual",
    });
    await reportManual("manual POST r307", res);
  }

  // --- manual mode: a non-redirect (200) response is unaffected ---
  {
    const res = await fetch(originA + "/final", { redirect: "manual" });
    await reportResponse("manual final200", res);
  }

  // --- error mode: rejects the moment a redirect status is seen ---
  await reportThrow("error r302", () => fetch(originA + "/r302", { redirect: "error" }));
  await reportThrow("error r307", () => fetch(originA + "/r307", { redirect: "error" }));

  // --- error mode: a non-redirect (200) response does not throw ---
  {
    const res = await fetch(originA + "/final", { redirect: "error" });
    await reportResponse("error final200", res);
  }

  serverA.close();
  serverB.close();
}

main();

/*
@covers
crates/perry-stdlib/src/fetch/turnloop_bridge.rs:
  - dispatch
  - dispatch_inputs
  - engine_redirect
  - store
  - failure_for
crates/perry-stdlib/src/turnloop_client/exchange.rs:
  - on_end
crates/perry-stdlib/src/fetch/request_handle.rs:
  - resolve_fetch_inputs
crates/perry-stdlib/src/fetch/mod.rs:
  - js_fetch_with_options
crates/perry-runtime/src/object/global_fetch.rs:
  - js_fetch_set_pending_redirect
  - js_fetch_take_pending_redirect
  - global_this_fetch_thunk
*/
