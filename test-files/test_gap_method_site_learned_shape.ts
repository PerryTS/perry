// Class-method call sites learn receiver shapes that are not the class's birth
// shape (fields added on one constructor path, fields added later, overflow
// storage). Every fact a learned shape stands for must still be re-checked:
// own shadowing methods, accessors, prototype changes, runtime method
// replacement, subclasses, and receivers moved by the collector.

class Tok {
  [key: string]: any;
  constructor(text: string, many: boolean) {
    this.text = text;
    this.pos = -1;
    this.count = 0;
    if (many) {
      this.a = 1; this.b = 2; this.c = 3; this.d = 4; this.e = 5;
      this.f = 6; this.g = 7; this.h = 8; this.i = 9; this.j = 10;
    }
  }
  step(): number { this.pos++; return this.pos < this.text.length ? this.text.charCodeAt(this.pos) : -1; }
  tag(): string { return "proto"; }
  run(): number { let cp = 0; while ((cp = this.step()) >= 0) this.count += cp & 1; return this.count; }
}

function callTag(t: Tok): string { return t.tag(); }

const text = "<a href=x>hello</a>".repeat(20);
let total = 0;
for (let round = 0; round < 50; round++) {
  total += new Tok(text, round % 2 === 0).run();
  total += new Tok(text, true).run();
}
console.log("run total", total);

// Warm the tag() site with a many-field receiver, then vary what shadows it.
const warm = new Tok(text, true);
let s = "";
for (let i = 0; i < 1000; i++) s = callTag(warm);
console.log("warm", s);

const ownFn = new Tok(text, true);
ownFn.tag = () => "own-fn";
console.log("own fn", callTag(ownFn), callTag(warm));

const ownBefore = new Tok(text, true);
(ownBefore as any).tag = function () { return "own-before-" + this.pos; };
for (let i = 0; i < 3; i++) s = callTag(ownBefore);
console.log("own before", s);

const getter = new Tok(text, true);
Object.defineProperty(getter, "tag", { get() { return () => "getter"; }, configurable: true });
console.log("accessor", callTag(getter), callTag(warm));
delete (getter as any).tag;
console.log("after delete", callTag(getter));

const reproto = new Tok(text, true);
Object.setPrototypeOf(reproto, { tag() { return "other-proto"; } });
console.log("setPrototypeOf instance", callTag(reproto), callTag(warm));

class Sub extends Tok {
  tag(): string { return "sub"; }
}
const sub = new Sub(text, true);
for (let i = 0; i < 3; i++) s = callTag(sub);
console.log("subclass", s, callTag(warm));

// A subclass the compiler cannot see (a mixin over an unknown base) reaches
// the same site: its instances must never take the declared class's body.
function mixin(B: any): any {
  return class extends B { tag(): string { return "mixed"; } };
}
const Mixed = mixin(Tok);
let mixedOut = "";
for (let i = 0; i < 5; i++) mixedOut += callTag(new Mixed(text, i % 2 === 0)) + ",";
console.log("mixin subclass", mixedOut, callTag(warm));

// Moving collections between calls: the learned word holds no address.
let keep: any[] = [];
let acc = 0;
for (let i = 0; i < 20000; i++) {
  keep.push({ i, s: "x" + i, arr: [i, i + 1] });
  if (keep.length > 200) keep = [];
  const t = i % 3 === 0 ? warm : new Tok("ab", i % 2 === 0);
  acc += callTag(t).length;
}
console.log("after churn", acc, callTag(warm));

// Replacing the prototype method retires every learned call of that name.
const before = callTag(warm);
Tok.prototype.tag = function () { return "replaced"; };
console.log("replaced", before, callTag(warm), callTag(new Tok(text, false)), callTag(sub));
