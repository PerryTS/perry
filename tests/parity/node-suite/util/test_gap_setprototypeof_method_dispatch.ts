// Prototype changes elsewhere in the program must not slow down or change
// `this.m()` dispatch on unrelated classes, and prototype changes that DO
// concern a class's instances or its chain must be observed.
import util from "node:util";

function Base(this: any) {}
Base.prototype.hello = function () { return "base-hello"; };
function Sub(this: any) {}
Object.setPrototypeOf(Sub.prototype, Base.prototype);
function Legacy(this: any) {}
util.inherits(Legacy, Base);
const literal: any = { a: 1 };
literal.__proto__ = { b: 2 };
const dict = { x: 1 };
Object.setPrototypeOf(dict, null);

class Tok {
  pos = 0;
  text: string;
  constructor(text: string) { this.text = text; }
  next(): number { return this.pos < this.text.length ? this.text.charCodeAt(this.pos++) : -1; }
  kind(): string { return "tok"; }
  run(): number { let n = 0; while (this.next() >= 0) n++; return n; }
}
let n = 0;
for (let i = 0; i < 200; i++) n += new Tok("abcdefgh".repeat(10)).run();
console.log("tok", n, new (Sub as any)().hello(), new (Legacy as any)().hello(), literal.b, Object.getPrototypeOf(dict));

// A class prototype relinked: inherited names follow the new chain.
class A { who(): string { return "A"; } shared(): string { return "A.shared"; } }
class B extends A { own(): string { return "B.own"; } }
const b = new B();
const call = (x: B) => x.who() + "/" + x.own();
let s = "";
for (let i = 0; i < 100; i++) s = call(b);
console.log("before relink", s, b.shared());
Object.setPrototypeOf(B.prototype, { who() { return "Z"; }, shared() { return "Z.shared"; } });
console.log("after relink", call(b), b.shared());

// An instance relinked to an unrelated object.
const t = new Tok("xy");
const k = (x: Tok) => x.kind();
for (let i = 0; i < 100; i++) s = k(t);
Object.setPrototypeOf(t, { kind() { return "other"; } });
console.log("instance relink", s, k(t), k(new Tok("z")));

// A class extending a plain function whose prototype is relinked later.
function Root(this: any) {}
Root.prototype.greet = function () { return "root"; };
class Leaf extends (Root as any) { leaf(): string { return "leaf"; } }
const leaf: any = new Leaf();
console.log("leaf before", leaf.greet(), leaf.leaf());
Object.setPrototypeOf(Root.prototype, { greet2() { return "relinked"; } });
console.log("leaf after", leaf.greet(), leaf.greet2(), leaf.leaf());
