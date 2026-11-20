// node:http's OutgoingMessage, ServerResponse and IncomingMessage
// prototypes: node's own keys, and program writes to them read back through
// an instance (a store, a new key, a delete, a defineProperty accessor).
import * as http from "node:http";
import { Readable } from "node:stream";

const SR: any = http.ServerResponse.prototype;
const OM: any = http.OutgoingMessage.prototype;
const IM: any = http.IncomingMessage.prototype;
console.log("OM", JSON.stringify(Reflect.ownKeys(OM).map(String)));
console.log("SR", JSON.stringify(Reflect.ownKeys(SR).map(String)));
console.log("IM", JSON.stringify(Reflect.ownKeys(IM).map(String)));
console.log("chain", Object.getPrototypeOf(SR) === OM, Object.getPrototypeOf(IM) === Readable.prototype);
for (const [n, p] of [["OM", OM], ["SR", SR]] as any) {
  for (const k of ["headersSent", "writableEnded", "socket", "statusCode", "constructor", "end"]) {
    const d = Object.getOwnPropertyDescriptor(p, k);
    if (d) console.log(n, k, typeof d.get, typeof d.set, "value" in d ? typeof d.value : "-", d.enumerable, d.configurable);
  }
}
function readX(o: any): any { return o.x; }
function readCode(o: any): any { return o.statusCode; }
function readEnd(o: any): any { return o.end; }
const r: any = new http.ServerResponse({ method: "GET" } as any);
for (let i = 0; i < 3; i++) console.log("before", readX(r), readCode(r), typeof readEnd(r));
SR.x = 1;
SR.statusCode = 500;
for (let i = 0; i < 3; i++) console.log("after SR", readX(r), readCode(r));
OM.x = 2;
delete SR.x;
for (let i = 0; i < 3; i++) console.log("after OM", readX(r), readCode(r));
const origEnd = OM.end;
OM.end = function () { return "patched"; };
console.log("end patched", r.end(), readEnd(r) === OM.end);
OM.end = origEnd;
Object.defineProperty(OM, "y", { get() { return "gy"; }, configurable: true });
console.log("accessor", r.y, (r as any).y);
delete OM.x;
SR.statusCode = 200;
console.log("restored", readX(r), readCode(r));
IM.z = 3;
console.log("IM", IM.z, Object.keys(IM).includes("z"));
