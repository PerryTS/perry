// #685 / #1787: class-EXPRESSION statics on the fresh-object path.
//
// (1) `static { … }` blocks of a class expression returned from a factory
//     must run at each evaluation, with `this` bound to THAT evaluation's
//     fresh class object and the factory's captured locals visible.
// (2) A static METHOD called on the fresh class object (both the inline
//     `make(x).m()` form and the const-bound `const C = make(x); C.m()`
//     form) must bind `this` to the fresh object, so `this.<field>` reads
//     the per-evaluation own static field — not the shared template's
//     static-field global (which the fresh path never writes).

let log: string[] = [];
function makeExpr(tag: string) {
  return class {
    static viaBlock: string | null = null;
    static { log.push("block:" + tag); }
    static { this.viaBlock = tag; }
    static ast = tag;
    static viaThis() { return this.ast; }
  };
}

const A = makeExpr("A");
const B = makeExpr("B");
console.log("blocks:", log.join(","));
console.log("A.viaBlock:", A.viaBlock);
console.log("B.viaBlock:", B.viaBlock);

// Static-method `this` binding — const-bound receiver.
console.log("A.viaThis():", A.viaThis());
console.log("B.viaThis():", B.viaThis());
// Inline factory-result receiver.
console.log("inline:", makeExpr("C").viaThis());

// `this === receiver` identity inside the static body.
function makeIdent() {
  return class {
    static self() { return this === (globalThis as any).__IDENT ? "same" : "different"; }
  };
}
const I = makeIdent();
(globalThis as any).__IDENT = I;
console.log("identity:", I.self());

// Nested class DECLARATION block still runs once, in the factory (control).
log = [];
function makeDecl(tag: string) {
  class D { static t: string; static { log.push("decl:" + tag); D.t = tag; } }
  return D;
}
const D = makeDecl("D");
console.log("decl blocks:", log.join(","));
console.log("D.t:", D.t);

// Top-level class-expression block still runs at module init (control).
const E = class { static { console.log("toplevel block ran"); } };
void E;
