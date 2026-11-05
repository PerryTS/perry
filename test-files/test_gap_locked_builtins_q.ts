// DEC 250 extensions (owner 2026-11-03): (i) ADDS are locked too on
// Object.prototype, Function.prototype, %IteratorPrototype% and
// %ArrayIteratorPrototype%, and on Array.prototype's INDEX keys (compiled code
// relies on keys being absent there); named adds to Array.prototype stay
// writable, and `constructor` stays replaceable (owner ruling 11-03: global
// bindings and namespace members are not locked).
// Node diverges by design, so the expected output is committed
// (test-parity/expected/).
function attempt(row: string, run: () => string): void {
  try {
    console.log(row, "no throw", run());
  } catch (e) {
    const err = e as Error;
    console.log(row, "throw", err instanceof TypeError ? "TypeError" : err.name, err.message);
  }
}
const OP: any = Object.prototype;
const FP: any = Function.prototype;
const AP: any = Array.prototype;
const G: any = globalThis;
const arrayIterProto: any = Object.getPrototypeOf([][Symbol.iterator]());
const iterProto: any = Object.getPrototypeOf(arrayIterProto);
function helper(this: any[]): number { return this.length * 10; }

// (i) adds on the four add-locked prototypes
attempt("q1", () => { OP.x = 1; return String(({} as any).x); });
console.log("q1b absent:", "x" in OP, ({} as any).x);
attempt("q2", () => { FP.myHelper = helper; return String(typeof (function () {} as any).myHelper); });
attempt("q3", () => { Object.defineProperty(OP, "y", { value: 1, configurable: true, writable: true }); return String(({} as any).y); });
try { OP[Symbol.for("q4")] = 1; console.log("q4 no throw"); } catch (e) {
  const err = e as Error;
  console.log("q4", err instanceof TypeError ? "throw TypeError" : err.name, err.message);
}
attempt("q4b", () => { Object.defineProperty(OP, Symbol("q4b"), { value: 1, configurable: true }); return "defined"; });
attempt("q5", () => { ([][Symbol.iterator]() as any).__proto__.foo = 1; return String(arrayIterProto.foo); });
attempt("q6", () => { Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())).bar = 1; return String(iterProto.bar); });
attempt("q7", () => { iterProto.return = function () { return { done: true, value: undefined }; }; return String(typeof iterProto.return); });
console.log("q7b for-of still works:", [1, 2, 3].map((v) => v * 2).join(","), [...new Set([4, 5])].join(","));
// (i) Array.prototype: index keys locked, named adds and non-index numeric strings work
attempt("q8", () => { AP[5] = 1; return String(([] as any)[5]); });
console.log("q8b no hole filled:", ([, 7] as any)[0], 5 in AP);
attempt("q9", () => { AP["4294967295"] = 1; return String(AP["4294967295"]); });
attempt("q10", () => { AP.myHelper = helper; return String(([1, 2] as any).myHelper()); });
attempt("q11", () => { Object.defineProperty(AP, "0", { value: 1, configurable: true }); return String(([] as any)[0]); });
// `constructor` stays replaceable on every prototype (owner ruling 11-03 12:4xZ)
attempt("q12", () => { const c = OP.constructor; OP.constructor = c; return String(OP.constructor === Object); });
attempt("q13", () => { const c = FP.constructor; Object.defineProperty(FP, "constructor", { value: c, writable: true, configurable: true }); return String(FP.constructor === Function); });
// the iterator prototypes are member-locked too (replace) beside add-locked
attempt("q14", () => { arrayIterProto.next = function () { return { done: true, value: undefined }; }; return String(typeof arrayIterProto.next); });
attempt("q15", () => { iterProto[Symbol.iterator] = function () { return this; }; return String(typeof iterProto[Symbol.iterator]); });
// the interpreter runs the same funnels
const Run = Function;
attempt("q21", () => { Run("Object.prototype.z = 1")(); return String(({} as any).z); });
console.log("done", new Map([[1, "m"]]).get(1), [...[1, 2]].length, typeof Promise.resolve);
