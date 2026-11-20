// Retain lexical homes and exact keys across explicit moving collection windows.
declare function gc(): void;
function collect() { if (typeof gc === "function") gc(); }
function parent(label) { return class { m() { return label; } set x(v) { this.out = label + v; } static m() { return label; } }; }
function child(Base) { return class extends Base {
  m() { return super.m(); }
  arrow() { return () => super.m(); }
  write(v) { super.x = (collect(), v); return this.out; }
  keyed(key) { return super[key]; }
  static m() { return super.m(); }
}; }
const P = parent("P"), Q = parent("Q");
const A = child(P), B = child(Q);
const a = new A(), b = new B(), arrow = a.arrow(), am = A.m, bm = B.m;
class Names { ["\uD800"]() { collect(); return 7; } ["\uFFFD"]() { return 8; } }
const names = new Names();
class Weak extends WeakMap { #value = 9; constructor() { super((collect(), [])); } getValue() { collect(); return this.#value; } }
const weak = new Weak(), key = {};
weak.set(key, 10);
let result = "";
for (let i = 0; i < 10; i++) {
  collect();
  const converted = { toString() { collect(); return "m"; } };
  result = [a.m(), b.m(), arrow(), am(), bm(), a.write(i), a.keyed(converted).call(a), names["\uD800"](), names["\uFFFD"](), weak.getValue(), weak.get(key)].join(",");
}
console.log(result);
