// Two-phase realm, the global object first: the realm is populated in one
// pass, and the first read of a core name adopts it for the core entry. The
// adopted copy of the realm's word has to follow the global when a collection
// moves it (it is promoted at the first minor collection): core reads after
// garbage must still see the bound slots.
const g: any = globalThis;
console.log("first", typeof g.Intl, typeof g.WeakMap, typeof g.undeclaredRealmName);
console.log("core", typeof Promise, typeof Map, [1, 2, 3].length, typeof Error);
class Cell {
  next: Cell | null = null;
  v: number;
  constructor(v: number) { this.v = v; }
}
function churn(n: number): number {
  let head: Cell | null = null;
  let s = 0;
  for (let i = 0; i < n; i++) {
    const c = new Cell(i);
    c.next = i % 64 ? head : null;
    head = c;
    s += c.v & 7;
  }
  return s;
}
let total = 0;
for (let round = 0; round < 40; round++) {
  total += churn(50000);
  if (typeof Promise !== "function" || typeof Map !== "function" || typeof Array !== "function") {
    console.log("lost at round", round, typeof Promise, typeof Map, typeof Array);
    break;
  }
}
const kept: any[] = [];
for (let i = 0; i < 20000; i++) kept.push({ i, s: "x" + i, a: [i] });
console.log("after", total, kept.length, typeof Promise, typeof Map, typeof Symbol.iterator, typeof Error);
console.log("values", Promise.resolve(1) instanceof Promise, new Map([[1, 2]]).size, Array.isArray(kept), new Error("e").message);
console.log("same", g.Promise === Promise, g.Map === Map, typeof globalThis.Promise, Object.keys({ a: 1 }).length);
