// Two-phase realm: the first touch of the realm is a RegExp instance (the
// core: %RegExp.prototype%). Then the same built-in is read before and after
// the realm is completed, and must be one object.
const re = /(\d+)-(\d+)/g;
const text = "10-20 30-40";
const found: string[] = [];
for (let m = re.exec(text); m !== null; m = re.exec(text)) found.push(m[1] + "+" + m[2] + "@" + m.index);
console.log("regexp", found.join(" "), re.lastIndex, re.flags, re.source);

const beforeMap = Map;
const beforeArrayProto = Array.prototype;
const beforeError = Error;
const beforeIntl = Intl; // completes the realm (a phase-B name)
const beforeWeakMap = WeakMap;
const g: any = globalThis;
console.log("identity core", beforeMap === Map, beforeMap === g.Map, beforeArrayProto === g.Array.prototype, beforeError === g["Error"]);
console.log("identity rest", beforeIntl === Intl, beforeIntl === g.Intl, beforeWeakMap === g.WeakMap, beforeWeakMap === WeakMap);
console.log("identity members", Intl.NumberFormat === g.Intl.NumberFormat, WeakMap.prototype === Object.getPrototypeOf(new WeakMap()));
console.log("identity functions", parseInt === g.parseInt, Number.parseFloat === parseFloat, setTimeout === g.setTimeout, structuredClone === g.structuredClone);
console.log("identity proto", Object.getPrototypeOf(re) === RegExp.prototype, RegExp === g.RegExp, re.constructor === RegExp);
console.log("globalThis", g === globalThis, g.globalThis === g, typeof g.global, g.global === g);
const names = Object.getOwnPropertyNames(globalThis);
const wanted = ["Array", "Atomics", "FinalizationRegistry", "Intl", "Map", "Proxy", "RegExp", "WeakMap", "WeakRef", "WeakSet", "WebAssembly", "PerformanceEntry", "PerformanceMark", "PerformanceObserver", "performance", "parseInt", "undefined", "NaN"];
console.log("declared", wanted.filter((n) => names.includes(n)).length, wanted.length);
console.log("unique", new Set(names).size === names.length);
