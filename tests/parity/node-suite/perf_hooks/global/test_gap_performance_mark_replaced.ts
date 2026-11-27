// The User Timing members of the global `performance` (`mark`, `measure`,
// ...) can be replaced by a harness or polyfill, and every later call must
// run the replacement; so must a program that replaces
// `globalThis.performance` itself.
const m = performance.mark("a");
console.log(m.name, m.entryType, typeof performance.measure("ab", "a").duration);

(performance as any).mark = function (n: any) { return "replaced " + n; };
console.log(performance.mark("b"), performance.getEntriesByName("b").length);
delete (performance as any).mark;
console.log(performance.mark("c").name);

const real = performance;
(globalThis as any).performance = { mark(n: any) { return "program " + n; } };
console.log(performance.mark("d"), real.getEntriesByName("d").length);
