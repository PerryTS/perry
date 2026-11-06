// DEC 250 on the generator prototypes: the members the realm image gives
// %GeneratorPrototype% and %AsyncGeneratorPrototype% (`next`, `return`,
// `throw`, `@@toStringTag`; not `constructor`) are never written by the
// program. Replacing, deleting or redefining one, or relinking the object, is
// a TypeError at the store (the locked-prototype path of every built-in
// prototype); adding a member works. A generator function's own `prototype`
// object is the program's: patchable, and replaceable. Node diverges by
// design, so the expected output is committed (test-parity/expected/).
function* gen() { yield 1; yield 2; }
async function* agen() { yield 1; }
const GP: any = Object.getPrototypeOf(Object.getPrototypeOf(gen()));
const AGP: any = Object.getPrototypeOf(Object.getPrototypeOf(agen()));
function attempt(row: string, run: () => string): void {
  try {
    console.log(row, "no throw", run());
  } catch (e) {
    const err = e as Error;
    console.log(row, "throw", err instanceof TypeError ? "TypeError" : err.name, err.message);
  }
}
function drain(it: Iterator<number>): string {
  const out: number[] = [];
  for (let r = it.next(); !r.done; r = it.next()) out.push(r.value);
  return out.join(",");
}

// (1) replace, delete, redefine an image member
attempt("1", () => { GP.next = function () { return { value: 42, done: true }; }; return drain(gen()); });
console.log("1b next is the built-in:", drain(gen()));
attempt("2", () => { delete GP.return; return String(typeof GP.return); });
attempt("3", () => { Object.defineProperty(GP, "throw", { value: 1 }); return String(typeof GP.throw); });
attempt("3b", () => { Object.defineProperty(GP, Symbol.toStringTag, { value: "X" }); return String(GP[Symbol.toStringTag]); });
// (4) relink the prototype
attempt("4", () => { Object.setPrototypeOf(GP, null); return "relinked"; });
console.log("4b chain intact:", typeof gen()[Symbol.iterator] === "function", [...gen()].join(","));
// (5) adding a member works, and keeps working
attempt("5", () => { GP.myHelper = function (this: Iterator<number>) { return "helper " + drain(this); }; return (gen() as any).myHelper(); });
attempt("5b", () => { GP.myHelper = function () { return "second"; }; return (gen() as any).myHelper(); });
// (6) `constructor` stays patchable (it is configurable, not writable)
attempt("6", () => {
  const o = GP.constructor;
  Object.defineProperty(GP, "constructor", { value: 7, configurable: true });
  const r = String(GP.constructor);
  Object.defineProperty(GP, "constructor", { value: o, configurable: true });
  return r + " " + String(GP.constructor === o);
});
// (7) the generator function's own prototype is the program's
attempt("7", () => { (gen.prototype as any).next = function () { return { value: "mine", done: false }; }; return String(gen().next().value); });
attempt("7b", () => { delete (gen.prototype as any).next; return drain(gen()); });
attempt("7c", () => {
  function* h() { yield 1; }
  (h as any).prototype = { next() { return { value: "replaced", done: false }; } };
  return String((h() as any).next().value);
});
// (8) the async generator prototype is locked the same way
attempt("8", () => { AGP.next = function () { return Promise.resolve({ value: 0, done: true }); }; return "replaced"; });
attempt("8b", () => { delete AGP.return; return "deleted"; });
attempt("8c", () => { AGP.added = 3; return String(AGP.added); });
agen().next().then((r) => console.log("8d async next is the built-in:", r.value, r.done));
