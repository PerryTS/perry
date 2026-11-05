// Field initializers of a class with an OWN constructor extending a DYNAMIC
// parent (the mixin pattern) must run exactly once, after super() returns.
// Pre-fix, the standalone-ctor lowering staged them a second time BEFORE the
// body (mode `All` — the extends_expr case fell through the extends_name
// check): public initializers ran twice (silent double side effects), private
// ones threw "Cannot initialize a private field twice", and brands installed
// against the pre-super receiver made later reads throw "Cannot access
// private member from an object whose class did not declare it" — pi's
// startup crash.
function mixin(Base: any): any { return class extends Base { m(): number { return 1; } }; }
class Root { r = 0; constructor() { this.r = 1; } }
const Mixed: any = mixin(Root);
let pubRuns = 0;
class PubLeaf extends Mixed { q = (pubRuns++, 7); constructor() { super(); } }
const p = new PubLeaf();
console.log("pub:", p.q, p.m(), p.r, "runs:", pubRuns);
class PrivLeaf extends Mixed { #s = 5; constructor() { super(); } get(): number { return this.#s; } }
console.log("priv:", new PrivLeaf().get());
class NoCtor extends Mixed { n = (pubRuns++, 9); }
console.log("noctor:", new NoCtor().n, "runs:", pubRuns);
