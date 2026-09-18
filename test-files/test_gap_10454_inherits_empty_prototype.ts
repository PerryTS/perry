// #10454: `http.ServerResponse` and `stream.Readable` subclasses built the
// classic pre-ES-class way — `util.inherits(Fn, Base)` /
// `Object.setPrototypeOf(Fn.prototype, Base.prototype)` plus
// `Base.call(this, ...)` in the constructor body, or even a plain
// `class Sub extends ServerResponse {}` — had none of the base's methods.
// `ServerResponse`/`Readable` are modeled as handle factories / own-property
// installers respectively; `Base.call(this, ...)` built the real
// handle/state and then DISCARDED it, leaving `this` a plain object with no
// `setHeader`/`writeHead`/`end` (or `push`/`pipe`/`on`). This is
// light-my-request's exact shape (`lib/response.js`, `lib/request.js`:
// `const http = require('http'); function Response(req) {
// http.ServerResponse.call(this, req) }`), which made fastify's
// `app.inject()` hang instead of settling.
//
// The native bases are reached via ESM imports here rather than a CJS
// `require()` — a CJS-required native http/stream class hits a SEPARATE,
// pre-existing gap under `PERRY_NO_AUTO_OPTIMIZE=1` (the gap harness's own
// compile mode; see `run_parity_tests.sh`): "node:http constructors reached
// through require are inert" (same mechanism as closed #8547), independent
// of this issue. See the PR body for how the CJS/npm-package shape was
// checked separately.
import * as http from "node:http";
import { Readable } from "node:stream";
import * as util from "node:util";

const req: any = { method: "GET", httpVersionMajor: 1, httpVersionMinor: 1, headers: {} };
const H: any = http;

// `http.ServerResponse.call(this, req)` — a literal member-expression
// chain, light-my-request's EXACT shape.
function Response(this: any, r: any) {
  H.ServerResponse.call(this, r);
}
util.inherits(Response as any, H.ServerResponse);

function SetProto(this: any, r: any) {
  H.ServerResponse.call(this, r);
}
Object.setPrototypeOf((SetProto as any).prototype, H.ServerResponse.prototype);

class Sub extends http.ServerResponse {}

// A local alias reaching the SAME bound export via a different heritage
// shape — the generic runtime `.call`/`.apply` dispatch path, not the
// static member-expression recognition above.
const AliasServerResponse = H.ServerResponse;
function ViaAlias(this: any, r: any) {
  AliasServerResponse.call(this, r);
}
util.inherits(ViaAlias as any, AliasServerResponse);

function Request(this: any, opts: any) {
  (Readable as any).call(this, opts);
}
util.inherits(Request as any, Readable as any);

const show = (label: string, o: any) =>
  console.log(
    label,
    "setHeader:",
    typeof o.setHeader,
    "writeHead:",
    typeof o.writeHead,
    "end:",
    typeof o.end,
  );

console.log("ServerResponse.prototype.setHeader:", typeof H.ServerResponse.prototype.setHeader);
show("new ServerResponse(req)         ", new H.ServerResponse(req));
show("member expr .call(this)         ", new (Response as any)(req));
show("setPrototypeOf + .call(this)    ", new (SetProto as any)(req));
show("class Sub extends ServerResponse", new (Sub as any)(req));
show("local alias .call(this)         ", new (ViaAlias as any)(req));

const res: any = new (Response as any)(req);
try {
  res.setHeader("x-a", "1");
  console.log(
    "res.setHeader(...) returned; getHeader:",
    typeof res.getHeader,
    res.getHeader("x-a"),
  );
} catch (e: any) {
  console.log("res.setHeader(...) threw:", e.message);
}

function runReadableCall(name: string): Promise<void> {
  return new Promise((resolve) => {
    const r: any = new (Request as any)({ read() {} });
    console.log(name, "push:", typeof r.push, "pipe:", typeof r.pipe, "on:", typeof r.on);
    let out = "";
    r.on("data", (c: any) => (out += c));
    r.on("end", () => {
      console.log(name, "data:", JSON.stringify(out));
      resolve();
    });
    r.push("hi");
    r.push(null);
  });
}

async function main() {
  await runReadableCall("util.inherits(Fn, Readable) + .call(this)");
}

main();
