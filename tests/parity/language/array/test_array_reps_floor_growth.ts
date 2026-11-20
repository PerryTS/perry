// The length floor (wpfacts `solve::arr_len`): an index store raises only
// the length's upper bound; a negative or fractional key changes nothing.
// Node-exact with the floor on, off and in check mode.

// 1. A growable floor: stores at i in [-2, 9] from a loop.
const a = new Array<number>(4).fill(0);
function put(i: number, v: number): void {
  a[i] = v;
}
for (let i = -2; i <= 9; i += 3) put(i, i * 10);
console.log(a[0], a[1], a[2], a[3], a[6] ?? "h", a[5] ?? "h", a.length);
console.log(Object.keys(a).join(","));
let s = 0;
for (let i = 0; i < a.length; i++) s += a[i] ?? 100;
console.log("sum", s);

// 2. Negative and fractional keys are named properties: no length change.
const b = [1, 2, 3];
function putAny(k: number, v: number): void {
  (b as any)[k] = v;
}
putAny(-1, 5);
putAny(1.5, 6);
putAny(-16, 7);
console.log(b.length, (b as any)[-1], (b as any)[1.5], (b as any)["-16"], b[0], b[2]);
console.log(Object.keys(b).join(","));
let t = 0;
for (let i = 0; i < b.length; i++) t += b[i];
console.log("b sum", t);

// killers' shape: keys in [-16, 16] on a 192-element array.
const killers = new Array<number>(192).fill(0);
function kill(ply: number, m: number): void {
  killers[ply * 2 - 16] = m;
  killers[ply * 2 + 1] = m + 1;
}
for (let ply = 0; ply < 8; ply++) kill(ply, ply * 3);
let ks = 0;
for (let i = 0; i < killers.length; i++) ks += killers[i];
console.log("killers", killers.length, ks, killers[1], killers[15], (killers as any)[-16]);

// 19. The append bound: 1664 pushes in a constructor loop, then a
// `length`-bounded sum.
class Table {
  keys: number[] = [];
  constructor() {
    for (let i = 0; i < 1664; i++) this.keys.push((i * 2654435761) % 1000003);
  }
  sum(): number {
    let s = 0;
    for (let i = 0; i < this.keys.length; i++) s += this.keys[i];
    return s;
  }
}
const tab = new Table();
console.log("table", tab.keys.length, tab.sum());
const z: number[] = [];
for (let i = 0; i < 2000; i++) z.push(i);
let zs = 0;
for (let i = 0; i < z.length; i++) zs += (i * 3000000) % 7;
console.log("z", z.length, zs, z[1500]);

// Over the dense bound: no birth length (finding 2).
const big = new Array<number>(2_000_000).fill(7);
console.log("big", big[0], big[1_999_999], big[2_000_000] ?? -1, big.length);
const sparse = new Array<number>(2_000_000);
console.log("sparse", sparse[5] ?? -1, sparse.length);
