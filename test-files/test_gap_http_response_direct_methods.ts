// Each native response operation must respect subclass and patched methods.
import * as http from "node:http";
const OM: any = http.OutgoingMessage.prototype;
const SR: any = http.ServerResponse.prototype;
const trace: string[] = [];
function R(this: any, req: any) { (http.ServerResponse as any).call(this, req); }
Object.setPrototypeOf(R.prototype, SR);
const original = { setHeader: OM.setHeader, write: OM.write, end: OM.end, writeHead: SR.writeHead };
function exercise(label: string, r: any) {
  console.log(label, r.setHeader("x-test", "a,b") === r, r.getHeader("x-test"));
  console.log(label, r.writeHead(201, "Created") === r, r.statusCode, r.statusMessage);
  console.log(label, typeof r.write("one"), r.end("two") === r, r.finished, r.headersSent);
}
exercise("base", new http.ServerResponse({ method: "GET" } as any));
exercise("derived", new (R as any)({ method: "GET" }));
for (const name of ["setHeader", "write", "end"] as const) {
  R.prototype[name] = function (this: any, a: any, b: any, c: any) {
    trace.push("sub:" + name);
    return original[name].call(this, a, b, c);
  };
}
R.prototype.writeHead = function (this: any, a: any, b: any, c: any) {
  trace.push("sub:writeHead");
  return original.writeHead.call(this, a, b, c);
};
exercise("overrides", new (R as any)({ method: "GET" }));
console.log("subcalls", trace.join(","));
trace.length = 0;
OM.setHeader = function (this: any, n: any, v: any) {
  trace.push("patched:setHeader");
  return original.setHeader.call(this, n, v);
};
SR.writeHead = function (this: any, a: any, b: any, c: any) {
  trace.push("patched:writeHead");
  return original.writeHead.call(this, a, b, c);
};
exercise("patched", new http.ServerResponse({ method: "GET" } as any));
console.log("patchedcalls", trace.join(","));
OM.setHeader = original.setHeader;
SR.writeHead = original.writeHead;
const own: any = new http.ServerResponse({ method: "GET" } as any);
Object.defineProperty(own, "setHeader", { configurable: true, get() { trace.push("own:get"); return original.setHeader; } });
console.log("own", own.setHeader("x-own", "ok") === own, own.getHeader("x-own"));
delete own.setHeader;
own.extra = 1; // An overflowed receiver must use the complete operation.
console.log("overflow", own.setHeader("x-after", "ok") === own, own.getHeader("x-after"));
try { original.setHeader.call({}, "x-test", "v"); } catch (e: any) { console.log("bad receiver", e.name); }
