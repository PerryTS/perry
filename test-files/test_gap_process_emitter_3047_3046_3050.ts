// Gap test: node:process EventEmitter argument handling.
//   #3046 — process.nextTick callback validation + trailing-arg forwarding
//   #3047 — process.on/once listener validation + event-name coercion
//
// Compared byte-for-byte against `node --experimental-strip-types`.

function show(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "OK");
  } catch (err) {
    const e = err as { name: string; code?: string; message: string };
    console.log(label, "THROW", e.name, e.code, e.message.split("\n")[0]);
  }
}

// ---- #3046: nextTick callback validation ----
show("nt missing", () => process.nextTick());
show("nt number", () => process.nextTick(123));
show("nt object", () => process.nextTick({}));

// ---- #3047: listener validation ----
show("on missing listener", () => process.on("x"));
show("on number listener", () => process.on("x", 1));
show("once object listener", () => process.once("x", {}));

// ---- #3047: event-name coercion (number/null/object) fires the listener ----
let numFired = false;
process.on(123, () => {
  numFired = true;
});
const numEmitted = process.emit(123, "payload");
console.log("coerce number", numFired, numEmitted);
process.removeListener(123, () => {});

// ---- #3046: nextTick trailing-arg forwarding (direct + method value) ----
process.nextTick((...args: unknown[]) => console.log("tick direct", JSON.stringify(args)), "a", "b", "c");
const nt = process.nextTick;
nt((...args: unknown[]) => console.log("tick method", JSON.stringify(args)), "a", "b", "c");

// ---- listeners()/listenerCount() basic shape ----
function onFn(): void {}
process.removeAllListeners("__probe__");
process.on("__probe__", onFn);
const ls = process.listeners("__probe__");
console.log("listeners length", ls.length, ls[0] === onFn);
console.log("listenerCount", process.listenerCount("__probe__"));
process.removeAllListeners("__probe__");
