// #10508: the node:events module helpers (once, on, getEventListeners,
// get/setMaxListeners, listenerCount) and captureRejections /
// Symbol.for('nodejs.rejection') on a plain `new EventEmitter()`, plus
// EventEmitterAsyncResource, all through the emitter's own methods.
import { EventEmitter, once, on, getEventListeners, getMaxListeners, setMaxListeners, listenerCount, captureRejectionSymbol, EventEmitterAsyncResource } from "node:events";
const log = (...a: any[]) => console.log(...a);
async function main() {
  const e = new EventEmitter();
  setTimeout(() => e.emit("ready", 1, 2), 1);
  const v = await once(e, "ready"); log("once", JSON.stringify(v), e.listenerCount("ready"), e.listenerCount("error"));
  setTimeout(() => e.emit("error", new Error("bad")), 1);
  try { await once(e, "never"); } catch (err: any) { log("once rejected", err.message, e.listenerCount("never")); }
  const ac = new AbortController();
  const p = once(e, "late", { signal: ac.signal }); ac.abort();
  try { await p; } catch (err: any) { log("once aborted", err.name, e.listenerCount("late")); }
  const g = () => {}; e.on("gl", g);
  log("getEventListeners", getEventListeners(e, "gl").length, getEventListeners(e, "gl")[0] === g);
  setMaxListeners(3, e); log("maxl", getMaxListeners(e), e.getMaxListeners());
  log("lc", listenerCount(e, "gl"));
  // events.on async iterator
  const it = on(e, "tick");
  setTimeout(() => { e.emit("tick", 1); e.emit("tick", 2); e.emit("tick", 3); }, 1);
  let n = 0;
  for await (const ev of it) { log("on", JSON.stringify(ev)); if (++n === 3) break; }
  log("on cleaned", e.listenerCount("tick"));
  // captureRejections
  const cr = new EventEmitter({ captureRejections: true });
  cr.on("error", (er: any) => log("captured", er.message));
  cr.on("ev", async () => { throw new Error("async boom"); });
  cr.emit("ev");
  await new Promise((r) => setTimeout(r, 5));
  const cr2: any = new EventEmitter({ captureRejections: true });
  cr2[Symbol.for("nodejs.rejection")] = (er: any, ev: any) => log("custom rejection", er.message, ev);
  cr2.on("ev", async () => { throw new Error("c2"); });
  cr2.emit("ev");
  await new Promise((r) => setTimeout(r, 5));
  log("sym same", captureRejectionSymbol === Symbol.for("nodejs.rejection"));
  try { new EventEmitter({ captureRejections: 1 as any }); } catch (err: any) { log("bad cr", err.code); }
  // AsyncResource variant
  const ar = new EventEmitterAsyncResource({ name: "X" });
  log("ar", ar instanceof EventEmitter, typeof ar.asyncId, ar.asyncId > 0, typeof ar.triggerAsyncId, ar.asyncResource.eventEmitter === ar);
  ar.on("z", () => log("ar emit")); ar.emit("z"); ar.emitDestroy();
  try { new EventEmitterAsyncResource({ name: 5 as any }); } catch (err: any) { log("ar bad", err.code); }
  // instance methods passed around
  const em = new EventEmitter(); const emit = em.emit.bind(em); em.on("b", (x: any) => log("bound emit", x)); emit("b", 9);
  const onm = em.on; onm.call(em, "c", () => log("called on")); em.emit("c");
  log("done");
}
main();
