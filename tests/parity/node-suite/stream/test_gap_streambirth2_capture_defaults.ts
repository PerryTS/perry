import { EventEmitter } from "node:events";
import { Readable, Writable, Duplex } from "node:stream";
async function run() {
  const results: string[] = [];
  for (const enabled of [true, false, true]) {
    (EventEmitter as any).captureRejections = enabled;
    const objects = [new EventEmitter(), new EventEmitter({ captureRejections: false }),
      new Readable({ read() {} }), new Writable({ write(c, e, cb) { cb(); } }),
      new Duplex({ read() {}, write(c, e, cb) { cb(); } })];
    for (let i = 0; i < objects.length; i++) {
      const e = objects[i];
      if (!enabled) continue;
      e.on("error", (err: any) => results.push(String(i) + ":" + err.message));
      e.on("x", async () => { throw new Error("capture"); });
      e.emit("x");
    }
    await new Promise<void>(resolve => setTimeout(resolve, 10));
  }
  for (const invalid of [1, null, undefined, "true"]) {
    try { (EventEmitter as any).captureRejections = invalid; console.log("accepted"); }
    catch (error: any) { console.log(error.code); }
  }
  class R extends Readable {
    steps: string[] = [];
    _construct(cb: any) { this.steps.push("construct"); cb(); }
    _read() { this.steps.push("read"); this.push("x"); this.push(null); }
  }
  const r = new R();
  console.log("before:" + r.steps.join(","));
  const chunks = await r.toArray();
  console.log(r.steps.join(",") + ":" + chunks.join(","));
  class W extends Writable {
    steps: string[] = [];
    _construct(cb: any) { this.steps.push("construct"); cb(); }
    _write(c: any, e: any, cb: any) { this.steps.push("write"); cb(); }
    _final(cb: any) { this.steps.push("final"); cb(); }
  }
  const w = new W();
  await new Promise<void>(resolve => { w.on("finish", resolve); w.end("x"); });
  console.log(w.steps.join(","));
  let next = 0, returns = 0;
  const source = {
    next() { return ++next > 1 ? { done: true } : { done: false, value: 1 }; },
    return() { returns++; return { done: true }; },
    [Symbol.iterator]() { return this; }
  };
  const stream = Readable.from(source);
  const values = await stream.take(2).toArray();
  console.log(values.join(",") + ":" + returns + ":" + stream.destroyed);
  console.log(results.join("|"));
}
run();
