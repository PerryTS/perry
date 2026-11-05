// THR1: a key not known at compile time is evaluated at its place in the
// class body and installed there; a string result keeps its source position
// among the string keys, a symbol result goes with the symbols, an integer
// string goes first, and a known member that follows is not reordered.
function pick(s: string): any { return process.argv.length > 99 ? "never" : s; }
const sym = Symbol("dyn");
function pickSym(): any { return process.argv.length > 99 ? "never" : sym; }
class D {
  first() { return 1; }
  [pick("mid")]() { return "mid"; }
  *[Symbol.iterator]() { yield "it"; }
  [pickSym()]() { return "sym"; }
  [pick("7")]() { return "seven"; }
  [pick("first")]() { return "first-redefined"; }
  last() { return "last"; }
}
console.log(Reflect.ownKeys(D.prototype).map(String).join(","));
const d: any = new D();
console.log(d.first(), d.mid(), d[sym](), d[7](), d.last(), [...d].join());
class E {
  x = 1;
  y = 2;
  [pick("m")]() { return this.x + this.y; }
  sum() { return this.x * 10 + this.y; }
}
const e: any = new E();
e.x = 3;
console.log(e.m(), e.sum(), Object.keys(e).join(","));
