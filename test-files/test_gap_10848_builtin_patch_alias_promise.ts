// #10848 follow-up: two direct-call shapes that still ran the intrinsic after
// the built-in member had been replaced.

// 1. A replacement written through a local bound to the namespace.
const M: any = Math;
M.min = (..._a: any[]) => "MINE";
console.log("alias min", Math.min(1, 2), Math.max(1, 2));
const C: any = console;
const seen: string[] = [];
const origInfo = console.info;
C.info = (...a: any[]) => { seen.push(a.join(" ")); };
console.info("captured", 1);
console.info = origInfo;
console.log("alias info", seen);

// 2. Promise statics: codegen folded `Promise.resolve(…)` back to its
// intrinsic even though the call was lowered as a dynamic property call.
const origResolve = Promise.resolve;
(Promise as any).resolve = (v: any) => "resolve:" + v;
console.log("resolve", Promise.resolve(1));
(Promise as any).resolve = origResolve;
const origAll = Promise.all;
(Promise as any).all = (v: any[]) => "all:" + v.length;
console.log("all", Promise.all([1, 2]));
(Promise as any).all = origAll;
Promise.resolve(7).then((v) => console.log("restored", v));
