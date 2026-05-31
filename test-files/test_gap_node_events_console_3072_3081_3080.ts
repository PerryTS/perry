// #3072: node:events EventEmitter listener argument validation.
// on/addListener/once/prependListener/prependOnceListener/removeListener/off
// must throw TypeError [ERR_INVALID_ARG_TYPE] when the listener is not a
// function, and must accept + chain a valid function.
import { EventEmitter } from "node:events";

// Invalid listener values whose "Received …" rendering is byte-identical in
// Perry and Node: undefined, null, number, string, plain object, array.
function check(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "NO THROW");
  } catch (e: any) {
    console.log(label, e.name, e.code, "::", e.message);
  }
}

const e0 = new EventEmitter();
check("on undefined", () => e0.on("x", undefined as any));
check("on null", () => e0.on("x", null as any));
check("on number", () => e0.on("x", 1 as any));
check("on string", () => e0.on("x", "fn" as any));
check("on object", () => e0.on("x", {} as any));
check("on array", () => e0.on("x", [] as any));

const e1 = new EventEmitter();
check("addListener number", () => e1.addListener("x", 1 as any));
check("addListener object", () => e1.addListener("x", {} as any));

const e2 = new EventEmitter();
check("once null", () => e2.once("x", null as any));
check("once array", () => e2.once("x", [] as any));

const e3 = new EventEmitter();
check("prependListener undefined", () => e3.prependListener("x", undefined as any));
check("prependListener string", () => e3.prependListener("x", "fn" as any));

const e4 = new EventEmitter();
check("prependOnceListener number", () => e4.prependOnceListener("x", 1 as any));
check("prependOnceListener object", () => e4.prependOnceListener("x", {} as any));

const e5 = new EventEmitter();
check("removeListener null", () => e5.removeListener("x", null as any));
check("removeListener object", () => e5.removeListener("x", {} as any));

const e6 = new EventEmitter();
check("off number", () => e6.off("x", 1 as any));
check("off array", () => e6.off("x", [] as any));

// Valid function listeners still work and chain.
const ee = new EventEmitter();
let fired = 0;
const ret = ee.on("ping", () => {
  fired++;
});
console.log("on returns emitter:", ret === ee);
ee.emit("ping");
console.log("fired:", fired);

// once fires exactly once.
let onceFired = 0;
ee.once("o", () => {
  onceFired++;
});
ee.emit("o");
ee.emit("o");
console.log("onceFired:", onceFired);

// removeListener with a valid function removes it.
const handler = () => {
  fired++;
};
ee.on("rm", handler);
ee.removeListener("rm", handler);
ee.emit("rm");
console.log("after remove fired:", fired);

// addListener is an alias for on and also chains.
const ee2 = new EventEmitter();
console.log("addListener returns emitter:", ee2.addListener("a", () => {}) === ee2);
