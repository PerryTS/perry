// A heap ServerResponse is node's emitter: res.on/once/removeListener keep
// their listeners in res._events (EventEmitter.prototype's methods), and the
// response fires 'finish' through res.emit once its socket took the last
// chunk ('close' comes from the socket's close). Listener order, a listener
// added inside a handler, removeListener and once firing exactly once.
import * as http from "node:http";
import { Writable } from "node:stream";

const log: string[] = [];
function sock(async: boolean): any {
  return new Writable({
    write(_c: any, _e: any, cb: () => void) {
      if (async) setImmediate(cb);
      else cb();
    },
  });
}

// 1. ordering, once, removeListener, a listener added while emitting.
const r1: any = new http.ServerResponse({ method: "GET" } as any);
r1.assignSocket(sock(false));
const f1 = () => log.push("f1");
r1.on("finish", f1);
r1.once("finish", () => {
  log.push("once");
  r1.on("finish", () => log.push("late"));
});
r1.addListener("finish", () => log.push("f2"));
r1.removeListener("finish", f1);
r1.on("close", () => log.push("close1"));
console.log("r1 events", Object.keys(r1._events).join(","), r1._eventsCount, r1.listenerCount("finish"));
console.log("r1 proto events", Object.getPrototypeOf(r1._events));
r1.end("z", () => log.push("cb1"));
console.log("r1 after end", log.join(","), r1.listenerCount("finish"), r1.finished, r1.writableEnded);

// 2. no socket: node never finishes.
const r2: any = new http.ServerResponse({ method: "GET" } as any);
r2.on("finish", () => log.push("r2 finish"));
r2.end("x");
console.log("r2", r2.finished, r2.listenerCount("finish"));

// 3. the light-my-request shape: ServerResponse.call(this), an async
// socket, the subclass emits 'finish' itself and destroys.
function R(this: any, req: any) {
  (http.ServerResponse as any).call(this, req);
  this.assignSocket(sock(true));
  this.once("finish", () => log.push("R once finish"));
  this.once("close", () => log.push("R once close"));
}
Object.setPrototypeOf(R.prototype, http.ServerResponse.prototype);
R.prototype.end = function (this: any, d: any, e: any, cb: any) {
  if (d) this.write(d, e);
  (http.ServerResponse.prototype as any).end.call(this, cb);
  log.push("R emit");
  this.emit("finish");
  this.destroy();
};
const r3: any = new (R as any)({ method: "GET" });
r3.on("finish", () => log.push("R on finish"));
r3.on("close", () => log.push("R on close"));
r3.statusCode = 201;
r3.setHeader("x-a", "b");
r3.end("body", () => log.push("R endcb"));
log.push("sync");
console.log("r3 sync", r3.destroyed, r3.finished, r3.statusCode, r3.statusMessage, r3.headersSent);
setTimeout(() => {
  console.log(log.join(","));
  console.log("r3 later", r3._closed, r3.listenerCount("finish"), r3.listenerCount("close"));
}, 50);
