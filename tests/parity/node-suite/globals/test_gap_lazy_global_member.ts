// The global object read with a known key, an unknown key, destructuring, `in`, descriptors.
const g: any = globalThis;
console.log("known", g.parseInt("12"), typeof g.setTimeout, typeof g.WebAssembly);
console.log("computed", g["parse" + "Int"]("ff", 16), typeof g[["set", "Immediate"].join("")]);
const key = ["queue", "Microtask"].join("");
console.log("computed-fn", typeof g[key], g[key] === queueMicrotask);
const { isFinite: fin, parseFloat: pf, encodeURI: eu } = g;
console.log("destructure", fin(1), pf("3.5"), eu("a b"));
console.log("in", "fetch" in globalThis, "structuredClone" in globalThis, "WebAssembly" in globalThis);
for (const name of ["setTimeout", "parseInt", "WebAssembly", "fetch", "btoa", "isNaN"]) {
  const d = Object.getOwnPropertyDescriptor(globalThis, name)!;
  console.log("desc", name, typeof d.value, d.writable, d.enumerable, d.configurable);
}
const lazy = ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "setImmediate", "clearImmediate",
  "queueMicrotask", "structuredClone", "atob", "btoa", "fetch", "parseInt", "parseFloat", "isNaN", "isFinite",
  "encodeURI", "decodeURI", "encodeURIComponent", "decodeURIComponent", "escape", "unescape", "WebAssembly"];
console.log("keys", Object.keys(globalThis).filter((k) => lazy.includes(k)).sort().join(","));
console.log("names", Object.getOwnPropertyNames(globalThis).filter((k) => lazy.includes(k)).sort().join(","));
