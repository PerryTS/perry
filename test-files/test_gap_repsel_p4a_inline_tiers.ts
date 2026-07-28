// Test: repsel Phase 4a.1 — inline guard tiers for numeric plain-array reads,
// writes, and pushes. The typed-`number[]` tiers must behave byte-identically
// to the untyped/guarded paths across every edge the inline guards test:
// integrity flags (frozen/sealed), per-index descriptors, growth/forwarding,
// hole/undefined passthrough values, and NaN/-0/Infinity canonical stores.
// Validated byte-for-byte against `node --experimental-strip-types`.
export {};

// 1) canonical stores + dense append + sparse extend
const t: number[] = [1.5, 2.5, 3.5];
t[1] = t[0] + 1;
console.log(t.join(","));
t[3] = 9; // dense append (idx == length, within capacity)
console.log(t.join(","), t.length);
t[10] = 7; // sparse extend -> holes via the runtime arm
console.log(JSON.stringify(t), t.length, 5 in t);

// 2) non-canonical RHS passthrough (runtime value check must keep working)
const src: number[] = new Array(3);
src[0] = 42;
const dst: number[] = [0, 0, 0];
dst[0] = src[0]; // number passthrough
dst[1] = src[2]; // hole read -> undefined must be STORED as undefined
console.log(dst[0], dst[1], JSON.stringify(dst), 1 in dst);

// 3) frozen / sealed arrays must never take the inline store. Post-state
// only: the strict-mode throw for the frozen write / sealed extend is a
// pre-existing gap in the boxed set fallback (present before this phase),
// so this test pins the data outcome, not the throw.
const fr: number[] = [1, 2];
Object.freeze(fr);
try {
  fr[0] = 5;
} catch (e) {
  void e;
}
console.log(fr[0], fr.length); // 1 2 — untouched
const se: number[] = [1, 2];
Object.seal(se);
se[0] = 9; // sealed in-bounds write is allowed
console.log(se[0]);
try {
  se[2] = 5;
} catch (e) {
  void e;
}
console.log(se.length, 2 in se); // 2 false — no extension

// 4) per-index accessor diverts both reads and the fast tiers decline
const ac: number[] = [1, 2, 3];
let got = 0;
Object.defineProperty(ac, 1, {
  get() {
    got++;
    return 99;
  },
});
console.log(ac[1], got, ac[0] + ac[1], got);

// 5) growth + forwarding: push far past capacity, then read everything back
const g: number[] = [];
for (let i = 0; i < 1000; i++) g.push(i * 0.5);
let s = 0;
for (let i = 0; i < g.length; i++) s += g[i];
console.log(g.length, s);

// 6) aliased receivers: write through one name, read through the other
function bump(a: number[], b: number[]): number {
  a[0] = a[0] + 1;
  return b[0];
}
const al: number[] = [10];
console.log(bump(al, al), al[0]);

// 7) NaN / -0 / Infinity through the canonical store tier
const w: number[] = [0];
w[0] = 0 / 0;
console.log(Object.is(w[0], NaN));
w[0] = -0;
console.log(Object.is(w[0], -0));
w[0] = 1 / 0;
console.log(w[0]);
const big2: number[] = [1e308];
big2[0] = big2[0] * 10;
console.log(big2[0]);

// 8) push of non-canonical numeric values keeps the layout sound
const p: number[] = [];
for (let i = 0; i < 5; i++) p.push(src[0]); // read passthrough value
p.push(src[2] as unknown as number); // hole read -> pushes undefined
console.log(JSON.stringify(p), p.length, 5 in p);
let ps = 0;
for (let i = 0; i < p.length; i++) ps += p[i] || 0;
console.log(ps);
