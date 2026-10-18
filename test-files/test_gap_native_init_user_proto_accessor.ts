// A native constructor run on `this` (EventEmitter.call, ServerResponse.call,
// OutgoingMessage.call) performs node's ordinary [[Get]]/[[Set]] on its
// fields: an accessor on a user prototype level between the instance and the
// native prototype sees them, before and after an instance was born.
import * as http from "node:http";
import * as util from "node:util";
import { EventEmitter } from "node:events";

const req = { method: "GET", httpVersionMajor: 1, httpVersionMinor: 1, headers: {} };

function probe(name: string, base: any, key: string, call: (o: any) => void, getv: any, before: boolean) {
  function Q(this: any) { call(this); }
  util.inherits(Q, base);
  const log: string[] = [];
  const define = () =>
    Object.defineProperty((Q as any).prototype, key, {
      set(v) { log.push("set:" + typeof v); },
      get() { log.push("get"); return getv; },
      configurable: true,
    });
  if (before) define();
  else { new (Q as any)(); define(); log.length = 0; }
  const o = new (Q as any)();
  console.log(name, key, getv, before, log.join(",") || "-",
    Object.prototype.hasOwnProperty.call(o, key), Object.keys(o).length);
}

for (const before of [true, false]) {
  for (const g of [1, 0]) {
    probe("ee", EventEmitter, "_maxListeners", (o) => EventEmitter.call(o), g, before);
    probe("ee", EventEmitter, "_events", (o) => EventEmitter.call(o), g, before);
    probe("sr", http.ServerResponse, "outputData", (o) => (http.ServerResponse as any).call(o, req), g, before);
    probe("sr", http.ServerResponse, "req", (o) => (http.ServerResponse as any).call(o, req), g, before);
    probe("sr", http.ServerResponse, "sendDate", (o) => (http.ServerResponse as any).call(o, req), g, before);
    probe("om", http.OutgoingMessage, "outputSize", (o) => (http.OutgoingMessage as any).call(o), g, before);
    probe("om", http.OutgoingMessage, "writable", (o) => (http.OutgoingMessage as any).call(o), g, before);
  }
}

// Plain receivers: the fields node's constructors create, in order.
function Plain(this: any) { (http.OutgoingMessage as any).call(this); }
util.inherits(Plain, http.OutgoingMessage);
console.log("om-keys", Object.keys(new (Plain as any)()).join("|"));
function Res(this: any) { (http.ServerResponse as any).call(this, req); }
util.inherits(Res, http.ServerResponse);
console.log("sr-keys", Object.keys(new (Res as any)()).join("|"));
const e: any = {};
EventEmitter.call(e, { captureRejections: true });
console.log("ee-keys", Object.keys(e).join("|"), typeof e._events);

// An object literal prototype (no native ancestor): same [[Set]].
let n = 0;
const lit = { set outputData(_v: any) { n++; } };
const viaLit: any = Object.create(lit);
(http.ServerResponse as any).call(viaLit, req);
console.log("literal-proto", n, Object.prototype.hasOwnProperty.call(viaLit, "outputData"));

// A getter-only accessor refuses the store: node's strict internals throw.
function RO(this: any) { (http.OutgoingMessage as any).call(this); }
util.inherits(RO, http.OutgoingMessage);
Object.defineProperty((RO as any).prototype, "outputSize", { get() { return 7; }, configurable: true });
try { new (RO as any)(); console.log("getter-only: no throw"); }
catch (err: any) { console.log("getter-only:", err instanceof TypeError); }
