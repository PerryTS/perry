// The module-level `events.*` helpers reached INDIRECTLY. `nm_dispatch_events`
// had arms only for `init` and `EventEmitterAsyncResource`, so a captured value,
// a type-erased receiver or a spread call fell through to `undefined` while the
// statically dispatched form went straight to the same FFI and was correct —
// `events.listenerCount(e, "x")` returned 2 and `const c = events.listenerCount;
// c(e, "x")` returned `undefined`.
import events, { EventEmitter } from "node:events";

const e = new EventEmitter();
e.on("x", () => {});
e.on("x", () => {});
e.on("y", () => {});

const dyn: any = events;
const captured = events.listenerCount;
const countArgs: [EventEmitter, string] = [e, "x"];

console.log("listenerCount static:", events.listenerCount(e, "x"));
console.log("listenerCount captured:", (captured as any)(e, "x"));
console.log("listenerCount dynamic:", dyn.listenerCount(e, "x"));
console.log("listenerCount spread:", events.listenerCount(...countArgs));
console.log("listenerCount other event:", dyn.listenerCount(e, "y"));
console.log("listenerCount absent event:", dyn.listenerCount(e, "nope"));

console.log("getEventListeners static:", events.getEventListeners(e, "x").length);
console.log("getEventListeners dynamic:", dyn.getEventListeners(e, "x").length);

console.log("getMaxListeners default:", events.getMaxListeners(e));
dyn.setMaxListeners(15, e);
console.log("after dynamic setMaxListeners:", dyn.getMaxListeners(e));
events.setMaxListeners(...([21, e] as [number, EventEmitter]));
console.log("after spread setMaxListeners:", events.getMaxListeners(e));

// A name with no arm must stay `undefined` rather than becoming a hard throw.
console.log("unknown helper:", String(dyn.definitelyNotAnEventsHelper));

async function asyncForms() {
  const a = new EventEmitter();
  setTimeout(() => a.emit("ping", 42), 5);
  console.log("once dynamic:", JSON.stringify(await dyn.once(a, "ping")));

  const b = new EventEmitter();
  setTimeout(() => b.emit("ping", 7), 5);
  console.log(
    "once spread:",
    JSON.stringify(await events.once(...([b, "ping"] as [EventEmitter, string]))),
  );

  // `events.on` is routed by the same bridge, but its async ITERATION is a
  // separate pre-existing gap — `for await (const v of events.on(e, "tick"))`
  // drops its first value in the STATIC form too, on a tree without any of
  // this change (`events/on/async-iterator-abort` and `events/on/validation`
  // are already red for the same reason). Asserting it here would test that
  // bug, not this one. `typeof` is all this fixture can honestly claim.
  const c = new EventEmitter();
  console.log("on dynamic returns object:", typeof dyn.on(c, "tick"));

  const ac = new AbortController();
  const disposable = dyn.addAbortListener(ac.signal, () => console.log("abort listener fired"));
  console.log("addAbortListener dynamic typeof:", typeof disposable);
  ac.abort();
}

asyncForms();
