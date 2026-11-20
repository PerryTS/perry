// #11300: a method call on a value whose STATIC type is EventEmitter must
// dispatch on the runtime receiver.
//
// A parameter annotated `EventEmitter` was registered as a native `events`
// instance, so `emitter.on(...)` lowered straight to the events provider's
// `js_event_emitter_on(handle, ...)`. The provider only knows its own emitter
// handles: for any other EventEmitter (a stream, a net.Socket, `process`, a
// user subclass) the call silently did nothing. mongodb's
// `onData(emitter: EventEmitter, ...)` is called with its Transform-based
// message stream, so the driver never saw a single server reply.
import type { EventEmitter } from "node:events";
import { EventEmitter as EE } from "node:events";
import { PassThrough, Transform, type TransformCallback } from "node:stream";
import net from "node:net";

class Sized extends Transform {
  constructor() {
    super({ readableObjectMode: true });
  }
  override _transform(c: Uint8Array, _e: unknown, cb: TransformCallback): void {
    this.push(c);
    cb();
  }
}
class UserEmitter extends EE {
  tag = "user";
}

// Every EventEmitter method, called through an `EventEmitter`-typed param.
function exercise(label: string, emitter: EventEmitter, fire: () => void): Promise<void> {
  const seen: string[] = [];
  const onData = (v: any) => seen.push("on:" + (v?.length ?? String(v)));
  const onceData = (v: any) => seen.push("once:" + (v?.length ?? String(v)));
  const chained = emitter.on("data", onData);
  emitter.once("data", onceData);
  emitter.prependListener("data", () => seen.push("prepend"));
  console.log(label, "chain returns receiver", chained === emitter);
  console.log(label, "listenerCount", emitter.listenerCount("data"));
  console.log(label, "listeners", emitter.listeners("data").length, emitter.rawListeners("data").length);
  console.log(label, "eventNames has data", emitter.eventNames().includes("data"));
  fire();
  return new Promise((done) => setTimeout(() => {
    console.log(label, "seen", seen.join(","));
    emitter.off("data", onData);
    console.log(label, "after off", emitter.listenerCount("data"));
    emitter.removeAllListeners("data");
    console.log(label, "after removeAll", emitter.listenerCount("data"));
    emitter.setMaxListeners(3);
    console.log(label, "maxListeners", emitter.getMaxListeners());
    done();
  }, 20));
}

async function main(): Promise<void> {
  const plain = new EE();
  await exercise("plain", plain, () => {
    console.log("plain", "emit returns", plain.emit("data", "abc"), plain.emit("nothing"));
  });
  const sized = new Sized();
  await exercise("transform", sized, () => {
    sized.write(Buffer.from("abcd"));
  });
  const pass = new PassThrough();
  await exercise("passthrough", pass, () => {
    pass.write(Buffer.from("xy"));
  });
  const user = new UserEmitter();
  await exercise("user subclass", user, () => {
    console.log("user subclass", "emit returns", (user as EventEmitter).emit("data", "hello"));
  });
  await exercise("process", process, () => {
    (process as EventEmitter).emit("data", "p");
  });

  // An emitter-typed arrow-function parameter, and an `(e: EventEmitter)`
  // helper handed a real socket.
  const count = (e: EventEmitter, name: string): number => e.listenerCount(name);
  const server = net.createServer((sock) => {
    sock.end("pong");
  });
  server.listen(0, "127.0.0.1", () => {
    const port = (server.address() as any).port;
    const client = net.connect(port, "127.0.0.1");
    const attach = (e: EventEmitter) => {
      e.on("data", (d: any) => console.log("socket data", String(d)));
      e.once("close", () => {
        console.log("socket closed");
        server.close();
      });
    };
    attach(client);
    console.log("socket listenerCount", count(client, "data"), count(client, "close"));
  });
}
main();
