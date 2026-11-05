// node's own keys of a ServerResponse, in node's order: the emitter's three,
// OutgoingMessage's 24 fields, ServerResponse's own; statusMessage and
// statusCode appended when written (writeHead, or the implicit header).
import * as http from "node:http";
import { Writable } from "node:stream";

function show(tag: string, o: any) {
  console.log(tag, JSON.stringify(Object.keys(o)));
  console.log(tag, "names", Object.getOwnPropertyNames(o).length, Object.keys(o).length);
}
const req: any = { method: "GET" };
const a: any = new http.ServerResponse(req);
show("new", a);
console.log("values", a.outputSize, a.writable, a.destroyed, a.sendDate, a._contentLength, a._hasBody,
  JSON.stringify(a._trailer), a.finished, a._headerSent, a._header, a._keepAliveTimeout,
  typeof a._onPendingData, a.req === req, a._sent100, a._expect_continue, Array.isArray(a.outputData));
console.log("inherited", a.statusCode, a.statusMessage, a.headersSent, a.writableEnded, a.closed,
  a.writableObjectMode, a.errored, a._eventsCount, a._maxListeners);
console.log("same nop", a._onPendingData === new http.ServerResponse(req)._onPendingData);
a.statusCode = 404;
show("status set", a);
a.writeHead(404, { "content-type": "text/plain" });
show("after writeHead", a);
console.log("head", a.statusCode, a.statusMessage, typeof a._header, a.headersSent, a._headerSent);
const h: any = new http.ServerResponse({ method: "HEAD" } as any);
console.log("head req body", h._hasBody);
const b: any = new http.ServerResponse(req);
b.assignSocket(new Writable({ write(_c: any, _e: any, cb: () => void) { cb(); } }));
b.end("hello");
show("after end", b);
console.log("end", b.statusCode, b.statusMessage, b.finished, b._headerSent, b._contentLength, b.writableEnded);

function Sub(this: any, r: any) {
  (http.ServerResponse as any).call(this, r);
  this.mine = 1;
}
Object.setPrototypeOf(Sub.prototype, http.ServerResponse.prototype);
const s: any = new (Sub as any)(req);
show("call(this)", s);
console.log("sub", s instanceof http.ServerResponse, s instanceof http.OutgoingMessage, s.statusCode);
class C extends (http.ServerResponse as any) {
  extra = 2;
}
const c: any = new (C as any)(req);
show("extends", c);
console.log("instanceof", a instanceof http.ServerResponse, a instanceof http.OutgoingMessage,
  c instanceof http.ServerResponse);
