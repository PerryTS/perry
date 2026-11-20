// #10848: replacing a built-in namespace method must be honored by DIRECT
// member calls, not only by reads / computed calls / `.call()`.

// Math / JSON / Reflect statics.
const mine = (..._a: any[]) => "MINE";
(Math as any).max = mine;
console.log("identity", (Math as any).max === mine);
console.log("direct", Math.max(1, 2));
console.log("computed", (Math as any)["max"](1, 2));
console.log("call", Math.max.call(null, 1, 2));
console.log("apply", Math.max.apply(null, [1, 2]));
console.log("spread", Math.max(...[1, 2]));
Object.defineProperty(Math, "max", { value: () => "MINE2", writable: true, configurable: true });
console.log("defineProperty", Math.max(1, 2));
console.log("sibling untouched", Math.min(1, 2), Math.abs(-3));

const origStringify = JSON.stringify;
(JSON as any).stringify = (v: any) => "<" + origStringify(v) + ">";
console.log("json", JSON.stringify({ a: 1 }));
(JSON as any).stringify = origStringify;
console.log("json restored", JSON.stringify({ a: 1 }));

// console capture: the OpenTUI shape (capture, forward, restore).
const out: string[] = [];
const origLog = console.log;
const origError = console.error;
console.error = (...a: any[]) => { out.push("E:" + a.join(",")); };
console.error("x", 1);
console.log = function (this: any, ...a: any[]) {
  out.push("L:" + a.join("|") + ":" + (this === console));
};
console.log("hello", 2);
console.log = origLog;
console.error = origError;
console.log("restored log", out);

// A wrapper that forwards to the original must not recurse into itself.
console.log = (...a: any[]) => origLog("[wrapped]", ...a);
console.log("through wrapper");
console.log = origLog.bind(console);
console.log("through bind");
console.log = origLog;

// Dynamic-key replacement of several methods at once.
const seen: string[] = [];
for (const m of ["warn", "info", "debug"]) {
  (console as any)[m] = (...a: any[]) => { seen.push(m + ":" + a.join(" ")); };
}
console.warn("w");
console.info("i");
console.debug("d");
console.log("dynamic keys", seen);

// A throwing override must not wedge later calls to the same method.
console.warn = () => { throw new Error("boom"); };
try {
  console.warn("x");
} catch (e: any) {
  console.log("caught", e.message);
}
console.warn = (...a: any[]) => { seen.push("after-throw:" + a.join(" ")); };
console.warn("again");
console.log("after throw", seen[seen.length - 1]);

// Replacing the whole namespace object.
const captured: string[] = [];
const realConsole = console;
(globalThis as any).console = { ...realConsole, log: (...a: any[]) => captured.push(a.join(" ")) };
console.log("into replacement", 1);
(globalThis as any).console = realConsole;
console.log("replacement captured", captured);

// Control: ordinary objects were never affected.
const o = { m() { return 1; } };
o.m = () => 2;
console.log("control", o.m());
