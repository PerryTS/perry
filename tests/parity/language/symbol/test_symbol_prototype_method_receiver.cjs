// A method a program adds to a primitive's prototype runs with that primitive
// as its receiver: a sloppy body sees the wrapper object, a strict one the
// primitive itself.
function sloppy() { return typeof this + (this instanceof Object ? ":wrap" : ":raw"); }
function strict() { "use strict"; return typeof this; }
function args(a, b) { return [typeof this, a, b].join(","); }

const values = [Symbol("s"), "a", 5, true, 10n];
for (const proto of [Symbol.prototype, String.prototype, Number.prototype, Boolean.prototype, BigInt.prototype]) {
  proto.mSloppy = sloppy;
  proto.mStrict = strict;
}
for (const value of values) {
  const dynamic = [value][0];
  console.log(typeof value, value.mSloppy(), value.mStrict(), dynamic.mSloppy(),
    dynamic.mStrict(), value["mSloppy"](), value["mStrict"]());
}

const sym = Symbol("t");
Symbol.prototype.args = args;
console.log(sym.args(1, "x"));
Symbol.prototype.self = function () { "use strict"; return this; };
console.log(sym.self() === sym);
console.log(sym.toString(), sym.valueOf() === sym);

Symbol.prototype.notAFunction = 1;
for (const name of ["notAFunction", "missing"]) {
  try {
    sym[name]();
  } catch (e) {
    console.log(name, e instanceof TypeError);
  }
}
