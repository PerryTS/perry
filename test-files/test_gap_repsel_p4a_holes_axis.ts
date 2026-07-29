// Test: repsel Phase 4a.2 — hole-tolerant numeric tier + HOLES-flag
// maintenance + inline sparse-extend growth (#6904 axis). `new Array(n)`
// mid-fill reads/writes, dense->sparse transitions, and every hole-default
// consumer form must stay byte-exact — including hole-vs-undefined
// observability through `in` / `Object.keys` / `JSON.stringify` and the
// -0 / NaN edges of the `|| 0` truthiness form.
// Validated byte-for-byte against `node --experimental-strip-types`.
export {};

// --- mid-fill histogram shape over a fresh holey array ---
const c: number[] = new Array(8);
c[3] = (c[3] || 0) + 1; // hole -> 1
c[3] = (c[3] || 0) + 1; // 1 -> 2
console.log(JSON.stringify(c), 3 in c, 0 in c);

// number-context reads over holes: |0, >>>0, arithmetic, compare
console.log(c[0] | 0, c[0] >>> 0, c[0] * 2, c[0] > 0, c[3] | 0);

// --- sparse extend keeps the invariant + observability ---
const s: number[] = [1, 2];
s[6] = 7; // gap 2..5 becomes holes
console.log(JSON.stringify(s), s.length, 4 in s, Object.keys(s).join(","));
let sum = 0;
for (let i = 0; i < s.length; i++) sum += s[i] || 0;
console.log(sum);
s[4] = (s[4] ?? 100) + 1; // hole -> 101
console.log(s[4], JSON.stringify(s));

// --- dense -> sparse -> reads through the hole-tolerant tier ---
const d: number[] = [10, 20, 30];
d[10] = 40;
console.log(d.length, d[5], d[10], JSON.stringify(d));
let t2 = 0;
for (let i = 0; i < d.length; i++) t2 += d[i] || 0;
console.log(t2);

// --- growth boundary: dense appends, then a write beyond capacity ---
const g: number[] = [1];
for (let i = 1; i <= 40; i++) g[i] = i;
console.log(g.length, g[40], g[17]);
g[100] = 5; // beyond capacity -> runtime grow + gap fill
console.log(g.length, g[99], 99 in g, g[100]);
let t3 = 0;
for (let i = 0; i < g.length; i++) t3 += g[i] || 0;
console.log(t3);

// --- NaN / -0 stored into a holey array; ||-class consumers stay exact ---
const w: number[] = new Array(4);
w[0] = NaN;
w[1] = -0;
console.log(w[0] || 9, Object.is(w[1] || 9, 9), Object.is(w[1] ?? 9, -0), w[2] ?? 9);
console.log(Object.is(w[0] ?? 5, NaN)); // stored NaN is NOT nullish

// --- explicit undefined store demotes; hole-vs-undefined stays observable ---
const u: number[] = new Array(3);
u[0] = 1;
(u as any)[1] = undefined;
console.log(JSON.stringify(u), 1 in u, 2 in u, u[1], u[2]);
console.log((u[1] || 3) + (u[2] || 4));

// --- iteration / join over holes after the fast tiers ran ---
console.log(w.join("|"));
console.log(s.join("|"));
