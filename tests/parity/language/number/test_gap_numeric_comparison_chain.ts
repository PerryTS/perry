// Shared slow comparisons must coerce at every reached leaf, in JS order.
function ranges(x: any): any {
  return x >= 10 && x <= 12 || x > 20 && x < 24 || x === 30;
}
function reversed(x: any): any {
  return 10 <= x && 12 >= x || 20 < x && 24 > x || 30 === x;
}
function two(x: any, y: any): any {
  return x < y && y >= x || x > y && y <= x;
}
function ifRange(x: any): any {
  if (x >= 10 && x <= 12 || x === 30) return "yes";
  return "no";
}
function ternary(x: any): any {
  return x >= 10 && x <= 12 || x === 30 ? "yes" : "no";
}
function loose(x: any): any {
  return x == 10 || x != 11 && x <= 12;
}
for (const x of [NaN, -0, 0, 10, 12, 13, 21, 24, 30, Infinity,
                 -Infinity, "10", "21", "30", "bad", "", null,
                 undefined, false, true, 10n, 21n, 30n, 9007199254740993n]) {
  console.log(ranges(x), reversed(x), ifRange(x), ternary(x), loose(x));
}
let calls = 0;
const changing: any = { valueOf() { calls++; return calls % 2 ? 11 : 100; } };
console.log("changing", ranges(changing), calls);
calls = 0;
console.log("reversed", reversed(changing), calls);
calls = 0;
console.log("if", ifRange(changing), calls);
calls = 0;
console.log("ternary", ternary(changing), calls);
let order = "";
const a: any = { valueOf() { order += "a"; return 11; } };
const b: any = { valueOf() { order += "b"; return 12; } };
console.log("two", two(a, b), order);
let strings = 0;
const stringObject: any = {
  valueOf() { strings++; return "11"; }
};
console.log("string-object", ranges(stringObject), strings);
let nanCalls = 0;
const nanObject: any = { valueOf() { nanCalls++; return NaN; } };
console.log("nan-object", ranges(nanObject), nanCalls);
let primitiveOrder = "";
const fallback: any = {
  valueOf() { primitiveOrder += "v"; return {}; },
  toString() { primitiveOrder += "s"; return "11"; }
};
console.log("fallback", ranges(fallback), primitiveOrder);
let hooks = 0;
const hooked: any = {
  [Symbol.toPrimitive](hint: any) { hooks++; console.log("hint", hint); return 11; }
};
console.log("hooked", ranges(hooked), hooks);
let thrown = 0;
const throwing: any = { valueOf() { thrown++; throw new Error("coerce"); } };
try { ranges(throwing); } catch (e) { console.log("throw", thrown); }
// These reads cannot be hoisted: coercion rewrites a binding through a closure.
let global: any;
const mutate: any = { valueOf() { global = 100; return 11; } };
global = mutate;
console.log("global", global >= 10 && global <= 12);
function mutable(): any {
  let local: any;
  local = { valueOf() { local = 100; return 11; } };
  return local >= 10 && local <= 12;
}
console.log("mutable", mutable());
// Logical test contexts must preserve operand values in value contexts.
console.log("operands", 0 && "unused", "" || "fallback", null ?? "nullish");
let seen = "";
function mark(s: any, value: any): any { seen += s; return value; }
if (mark("a", 0) && mark("b", 1) || mark("c", "ok")) seen += "d";
console.log("test-order", seen);

console.log("strings", two("10", "2"), two("bad", "2"));
function loops(x: any): any {
  let n = 0;
  while (x >= 10 && x <= 12 || x === 30) { n++; break; }
  do { n++; } while (x < 0 && x > -10 && n < 3);
  for (; x >= 10 && x <= 12 && n < 3; n++) { }
  return n;
}
console.log("loops", loops(11), loops("11"), loops(NaN));
