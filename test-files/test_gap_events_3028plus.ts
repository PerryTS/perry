import { EventEmitter } from "node:events";

// #3028: coerce non-symbol event names to strings.
const ee = new EventEmitter();
const counts: Record<string, number> = {};
for (const event of [123, null, undefined, {}, "x"]) {
  ee.on(event as any, () => {
    const k = String(event);
    counts[k] = (counts[k] ?? 0) + 1;
  });
}
ee.emit(123 as any);
ee.emit(null as any);
ee.emit(undefined as any);
ee.emit({} as any);
ee.emit("x" as any);
console.log("on/emit 123:", counts["123"]);
console.log("on/emit null:", counts["null"]);
console.log("on/emit undefined:", counts["undefined"]);
console.log("on/emit object:", counts["[object Object]"]);
console.log("on/emit x:", counts["x"]);
console.log("names:", ee.eventNames().sort().join("|"));

// #3029: listenerCount filter by listener.
const ee2 = new EventEmitter();
function a() {}
function b() {}
ee2.on("e", a);
ee2.on("e", b);
ee2.on("e", a);
console.log("lc total:", ee2.listenerCount("e"));
console.log("lc a:", ee2.listenerCount("e", a));
console.log("lc b:", ee2.listenerCount("e", b));
console.log("lc anon:", ee2.listenerCount("e", () => {}));

// #2933: MaxListenersExceededWarning over the limit (stderr).
const ee3 = new EventEmitter();
for (let i = 0; i < 11; i++) ee3.on("leak", () => {});
console.log("count over default:", ee3.listenerCount("leak"));

const ee4 = new EventEmitter();
ee4.setMaxListeners(2);
for (let i = 0; i < 3; i++) ee4.on("y", () => {});
console.log("count over custom:", ee4.listenerCount("y"));

const ee5 = new EventEmitter();
ee5.setMaxListeners(0);
for (let i = 0; i < 20; i++) ee5.on("z", () => {});
console.log("count disabled:", ee5.listenerCount("z"));

// #2933: removeAllListeners resets the per-event warning so a re-grow warns
// again (and the trace-warnings hint prints only once per process).
const ee6 = new EventEmitter();
ee6.setMaxListeners(2);
for (let i = 0; i < 3; i++) ee6.on("w", () => {});
ee6.removeAllListeners("w");
for (let i = 0; i < 3; i++) ee6.on("w", () => {});
console.log("count after reset:", ee6.listenerCount("w"));
