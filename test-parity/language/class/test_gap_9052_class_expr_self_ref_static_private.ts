// lru-cache's construction-gate pattern (minified into pi's bundle): a
// closure-nested NAMED class expression whose static method flips a static
// private through the lexical self-binding. Codegen hands the runtime the
// bare class REF as the receiver; the per-evaluation brand paths compared
// its (nonexistent) brand against the current evaluation's and threw
// "Cannot access private member from an object whose class did not declare
// it" — pi's startup crash. A self-ref receiver can only be emitted from
// inside the class's own body, where it denotes the current evaluation.
const make = (): any => class c {
  static #o = false;
  static create(): any { c.#o = true; const i = new c(); c.#o = false; return i; }
  ok = 0;
  constructor() { if (!c.#o) throw new TypeError("gate"); this.ok = 1; }
};
const A: any = make(); const B: any = make();
console.log("A:", A.create().ok, "B:", B.create().ok);
let caught = "";
try { new A(); } catch (e) { caught = "gated"; }
console.log("direct-new:", caught);
