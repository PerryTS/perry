// Gap test for #2640 (Response status/statusText validation + default)
// and #2643 (Request method normalization + GET/HEAD body rejection).
// Compared byte-for-byte against `node --experimental-strip-types`.
//
// Uses object literals at each `new Response`/`new Request` call site (the
// path the issues report). The runtime-object init path (e.g. a loop var)
// is a separate, pre-existing constructor-lowering gap.

function respErr(fn: () => void): void {
  try {
    fn();
    console.log("no throw");
  } catch (e: any) {
    console.log("respErr " + e.name + " " + e.message.split("\n")[0]);
  }
}

function methodErr(fn: () => void): void {
  try {
    fn();
    console.log("no throw");
  } catch (e: any) {
    console.log("err " + e.name + " " + e.message.split("\n")[0]);
  }
}

// --- #2640: Response default statusText + valid init ---
const r0 = new Response("x");
console.log("default statusText: " + JSON.stringify(r0.statusText));
console.log("default status: " + r0.status);

const r1 = new Response("x", {});
console.log("empty-init statusText: " + JSON.stringify(r1.statusText));

const r2 = new Response("x", {
  status: 201,
  statusText: "Created-ish",
  headers: { "x-test": "yes" },
});
console.log(
  "valid init: " +
    r2.status +
    " " +
    JSON.stringify(r2.statusText) +
    " " +
    r2.headers.get("x-test") +
    " " +
    r2.ok,
);

const r3 = new Response(null, { status: 204 });
console.log("null-body 204: " + r3.status + " " + JSON.stringify(r3.statusText));

// --- #2640: Response invalid init throws ---
respErr(() => {
  new Response("x", { status: 99 });
});
respErr(() => {
  new Response("x", { status: 600 });
});
respErr(() => {
  new Response("x", { statusText: "bad\nline" });
});
respErr(() => {
  new Response("x", { status: 204 });
});

// --- #2643: Request method normalization ---
console.log("method post => " + new Request("http://example.com", { method: "post" }).method);
console.log("method PATCH => " + new Request("http://example.com", { method: "PATCH" }).method);
console.log("method custom => " + new Request("http://example.com", { method: "custom" }).method);

// --- #2643: forbidden methods throw ---
methodErr(() => {
  new Request("http://example.com", { method: "connect" });
});
methodErr(() => {
  new Request("http://example.com", { method: "TRACE" });
});
methodErr(() => {
  new Request("http://example.com", { method: "TRACK" });
});

// --- #2643: GET/HEAD body rejection + POST body kept ---
methodErr(() => {
  new Request("http://example.com", { method: "GET", body: "x" });
});
methodErr(() => {
  new Request("http://example.com", { method: "HEAD", body: "x" });
});
const rp = new Request("http://example.com", { method: "POST", body: "x" });
console.log("body POST => " + rp.method);
