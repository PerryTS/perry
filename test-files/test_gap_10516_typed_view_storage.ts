// #10516: whether a typed array's elements follow its header is a fact of the
// typed array itself (its storage byte, mirrored in the kind-cache tag), not a
// process-wide count of live views. Owning arrays keep the inline element
// paths while views are alive; every view, including one created after an
// owning array was already read inline, must read and write its backing.

function rd32(a: Int32Array, i: number): number { return a[i] | 0; }
function rdU32(a: Uint32Array, i: number): number { return a[i]; }
function rdF64(a: Float64Array, i: number): number { return a[i] + 0.5; }
function wr32(a: Int32Array, i: number, v: number): void { a[i] = v; }
function rmwU32(a: Uint32Array, i: number, v: number): void { a[i] += v; }
function rdAny(a: any, i: number): any { return a[i]; }
function lenOf(a: Uint8Array): number { return a.length; }

// Owning arrays exercised before any view exists.
const own = new Int32Array(8);
const ownU = new Uint32Array(4);
const ownF = new Float64Array(4);
for (let i = 0; i < 8; i++) wr32(own, i, i * 3);
for (let i = 0; i < 4; i++) { ownU[i] = i + 1; ownF[i] = i * 1.5; }
let s = 0;
for (let i = 0; i < 8; i++) s += rd32(own, i);
console.log("before", s, rdU32(ownU, 3), rdF64(ownF, 2), rdAny(own, 7));

// A view over a fresh buffer, alive from here on.
const ab = new ArrayBuffer(32);
const v32 = new Int32Array(ab);
const v8 = new Uint8Array(ab, 4, 8);
const vU = new Uint32Array(ab, 16, 4);
for (let i = 0; i < 8; i++) wr32(v32, i, 100 + i);
rmwU32(vU, 0, 5);
console.log("views", rd32(v32, 1), v8[0], v8[3], rdU32(vU, 0), rdAny(v32, 2), lenOf(v8), v8.byteOffset);
v8[0] = 255; // aliases v32[1]'s low byte
console.log("alias", rd32(v32, 1), rdAny(v8, 0), new DataView(ab).getInt32(4, true));

// Owning arrays still read and write correctly while views are live.
s = 0;
for (let i = 0; i < 8; i++) { wr32(own, i, rd32(own, i) + 1); s += rd32(own, i); }
rmwU32(ownU, 2, 10);
console.log("owning", s, rdU32(ownU, 2), rdF64(ownF, 3), rdAny(ownF, 1), own.length);

// An owning array read inline FIRST, then given a `.buffer`: from then on its
// elements live in that buffer and a view over it aliases them.
const late = new Int32Array([7, 8, 9, 10]);
console.log("late-before", rd32(late, 2), rdAny(late, 3));
const lateView = new Int32Array(late.buffer, 4, 2);
wr32(lateView, 0, 80);
wr32(late, 2, 90);
console.log("late-after", rd32(late, 1), rd32(lateView, 1), rdAny(late, 1), rdAny(lateView, 0), late.length, lateView.length);

// subarray shares storage both ways.
const base = new Float64Array([1, 2, 3, 4, 5]);
const sub = base.subarray(1, 4);
sub[0] = 20;
base[3] = 40;
console.log("subarray", rdF64(base, 1), rdF64(sub, 2), rdAny(sub, 0), sub.length, sub.byteOffset);

// Out-of-bounds and lengths through views and owning arrays alike.
console.log("oob", rd32(v32, 99), rdAny(v32, 99), rdAny(own, -1), rdU32(vU, 7));
