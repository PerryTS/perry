// #10454: `http.ServerResponse` and `stream.Readable` subclasses built the
// pre-ES-class way — `util.inherits(Fn, Base)` or
// `Object.setPrototypeOf(Fn.prototype, Base.prototype)` plus
// `Base.call(this, …)` — and `class Sub extends ServerResponse` had none of
// the base's methods. `ServerResponse.prototype` was an empty object, and
// `Base.call(this, …)` built the native handle/state and discarded it. This
// is light-my-request's shape (`lib/response.js`, `lib/request.js`), which
// left fastify's `app.inject()` hanging.
import * as http from "node:http";
import { Readable } from "node:stream";
import * as util from "node:util";

const req: any = { method: "GET", httpVersionMajor: 1, httpVersionMinor: 1, headers: {} };
const H: any = http;

function Response(this: any, r: any) {
  H.ServerResponse.call(this, r);
}
util.inherits(Response as any, H.ServerResponse);

function SetProto(this: any, r: any) {
  H.ServerResponse.call(this, r);
}
Object.setPrototypeOf((SetProto as any).prototype, H.ServerResponse.prototype);

class Sub extends http.ServerResponse {}

// A local alias reaching the same export through a different heritage shape.
const AliasServerResponse = H.ServerResponse;
function ViaAlias(this: any, r: any) {
  AliasServerResponse.apply(this, [r]);
}
util.inherits(ViaAlias as any, AliasServerResponse);

// light-my-request's override: a prototype method that delegates to the base
// prototype's implementation with an explicit `this`.
let overrideRan = false;
(Response as any).prototype.writeHead = function (this: any, ...args: any[]) {
  overrideRan = true;
  return H.ServerResponse.prototype.writeHead.apply(this, args);
};

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
console.log("ServerResponse.prototype.end:", typeof H.ServerResponse.prototype.end);
show("new ServerResponse(req)          ", new H.ServerResponse(req));
show("util.inherits + .call(this)      ", new (Response as any)(req));
show("setPrototypeOf + .call(this)     ", new (SetProto as any)(req));
show("class Sub extends ServerResponse ", new (Sub as any)(req));
show("local alias .apply(this)         ", new (ViaAlias as any)(req));

const res: any = new (Response as any)(req);
console.log("instanceof ServerResponse:", res instanceof H.ServerResponse);
res.setHeader("x-a", "1");
console.log("getHeader after setHeader:", res.getHeader("x-a"));
console.log("hasHeader:", res.hasHeader("x-a"), res.hasHeader("x-b"));
console.log("setHeader returns this:", res.setHeader("x-c", "2") === res);
res.removeHeader("x-a");
console.log("getHeaderNames:", JSON.stringify(res.getHeaderNames()));
res.writeHead(201);
console.log("prototype override ran:", overrideRan);

const sub: any = new (Sub as any)(req);
sub.setHeader("content-type", "text/plain");
console.log("Sub getHeader:", sub.getHeader("content-type"));

try {
  H.ServerResponse.prototype.setHeader.call({}, "x", "y");
  console.log("setHeader on a plain object: returned");
} catch (e: any) {
  console.log("setHeader on a plain object threw:", e instanceof TypeError);
}

function Request(this: any, opts: any) {
  (Readable as any).call(this, opts);
}
util.inherits(Request as any, Readable as any);

const r: any = new (Request as any)({ read() {} });
console.log("util.inherits(Fn, Readable): push:", typeof r.push, "pipe:", typeof r.pipe, "on:", typeof r.on);
let out = "";
r.on("data", (c: any) => (out += c));
r.on("end", () => console.log("Readable data:", JSON.stringify(out)));
r.push("hi");
r.push(null);
