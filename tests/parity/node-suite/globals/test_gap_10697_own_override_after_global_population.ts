// #10697: populating globalThis installs the runtime's builtins onto its
// intrinsics. Those installs no longer arm the own-override guard that proven
// Map/Set/Date/Array builtin calls consult, so a program that touches a lazy
// global keeps the direct builtin call. A user's own override must still win
// afterwards, and the populated builtins must still be callable.

// Force the lazy population first (an intrinsic, an alias, a global function).
const g: any = globalThis;
console.log(typeof g.Uint8Array, typeof g.unescape, Number.parseFloat === g.parseFloat);
console.log(Uint8Array.from([1, 2, 3]).join(","), Number.parseFloat("2.5"), g.unescape("%41"));
console.log(Array.prototype.constructor === Array, [].constructor === Array);

// Plain builtin calls on proven receivers, after population.
const m = new Map<string, number>();
const cats = ["alpha", "beta", "gamma", "delta"];
for (let i = 0; i < 40; i++) m.set(cats[i & 3], (m.get(cats[i & 3]) || 0) + 1);
console.log([...m].join(";"), m.has("beta"), m.get("delta"));
const s = new Set<number>([1, 2, 3]);
console.log(s.has(2), s.size);
const d = new Date(0);
console.log(d.getTime(), d.toISOString());
const a = [3, 1, 2];
console.log(a.indexOf(1), a.includes(3), a.slice(1).join(","));

// Own overrides installed AFTER population must still beat the builtin.
const m2 = new Map<string, number>([["k", 1]]);
(m2 as any).get = (k: string) => "own-get:" + k;
console.log(m2.get("k"));
const s2 = new Set<number>([1]);
(s2 as any).has = (v: number) => "own-has:" + v;
console.log(s2.has(1));
const d2 = new Date(0);
(d2 as any).getTime = () => "own-getTime";
console.log(d2.getTime());
const a2 = [1, 2, 3];
(a2 as any).indexOf = (v: number) => "own-indexOf:" + v;
console.log(a2.indexOf(2));

// And a Map created before the override still uses the builtin.
console.log(m.get("alpha"), m.get("missing"));
