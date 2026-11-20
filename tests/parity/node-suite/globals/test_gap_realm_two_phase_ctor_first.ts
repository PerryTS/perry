// Two-phase realm: the first touch of the realm is a global read inside a
// constructor (a core name). Then `in`, descriptor queries, delete and
// writes of phase-B names: each sees the realm as if it had been complete.
class Board {
  cells: number[];
  made: number;
  constructor(n: number) {
    this.cells = new Array(n).fill(0);
    this.made = Number.isInteger(n) ? Math.max(n, 1) : 0;
  }
}
const board = new Board(4);
console.log("board", board.cells.length, board.made);

const g: any = globalThis;
console.log("in", "Intl" in globalThis, "WeakRef" in g, "Proxy" in g, "PerformanceMark" in g, "notAGlobal" in g);
console.log("own", Object.prototype.hasOwnProperty.call(g, "Atomics"), Object.hasOwn(g, "WeakSet"), Object.hasOwn(g, "notAGlobal"));
for (const name of ["WeakMap", "WeakRef", "Proxy", "FinalizationRegistry", "Atomics", "Intl", "WebAssembly", "Map", "parseInt", "NaN"]) {
  const d = Object.getOwnPropertyDescriptor(globalThis, name)!;
  console.log("descriptor", name, typeof d.value, d.writable, d.enumerable, d.configurable, "get" in d);
}
console.log("enumerable", Object.keys(globalThis).includes("Intl"), Object.keys(globalThis).includes("WeakMap"));
let seen = 0;
for (const k in globalThis) if (k === "Atomics" || k === "Proxy" || k === "WeakSet") seen++;
console.log("for-in", seen);

console.log("delete", delete g.Atomics, "Atomics" in g, typeof g.Atomics);
console.log("delete again", delete g.Atomics, Reflect.deleteProperty(g, "WeakRef"), "WeakRef" in g);
g.WeakSet = 5;
console.log("write", g.WeakSet, typeof g.WeakSet, Object.getOwnPropertyDescriptor(g, "WeakSet")!.enumerable);
Object.defineProperty(g, "FinalizationRegistry", { value: "replaced", writable: false, configurable: true, enumerable: false });
console.log("define", g.FinalizationRegistry, Object.getOwnPropertyDescriptor(g, "FinalizationRegistry")!.writable);
console.log("unaffected", typeof g.Intl, typeof g.WeakMap, typeof g.Proxy, new g.WeakMap() instanceof g.WeakMap);
