// A constructor's explicit object/function return overrides the allocated this.
class W1 { constructor() { return { kind: "obj" }; } }
const a: any = new W1();
if (typeof a !== "object" || a.kind !== "obj") throw new Error("W1: " + typeof a + " " + a.kind);

function makeFn() { const f: any = (x: string) => "F:" + x; f.tag = "T"; return f; }
class W2 { constructor() { return makeFn(); } }
const b: any = new W2();
if (typeof b !== "function" || b.tag !== "T" || b("x") !== "F:x") throw new Error("W2: " + typeof b);

class P { x: number; constructor(v: number) { this.x = v; } }
const p = new P(5);
if (p.x !== 5) throw new Error("P: " + p.x);
console.log("OK");
