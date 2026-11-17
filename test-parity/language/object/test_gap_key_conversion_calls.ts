// An object used as a property key or an index operand is converted by the
// operation itself: ToPropertyKey (ECMA-262 7.1.19) or ToIntegerOrInfinity
// (7.1.5), both through ToPrimitive (7.1.1). The key's own `@@toPrimitive`,
// `toString` or `valueOf` is program code the operation calls, also when
// nothing else names it. Unknown values come from `process.argv[2]`.
"use strict";
const arg = process.argv[2];
const log: string[] = [];
const key = (name: string, k: string): any => ({
  toString() {
    log.push("toString " + name);
    return (arg || "") + k;
  },
});
const num = (name: string): any => ({
  valueOf() {
    log.push("valueOf " + name);
    return Number(arg || 2);
  },
});

// 13.5.1.2 delete -> 13.3.3 EvaluatePropertyAccessWithExpressionKey.
const o1: any = { a: 1, b: 2 };
delete o1[key("delete", "a")];
const o2: any = { a: 1 };
const r = delete o2[key("delete value", "a")];
console.log(Object.keys(o1).join(), r, "a" in o2);

// 15.4.4 / 15.7.10: computed class method and field names.
class C {
  [key("class method", "m")]() {
    return 4;
  }
  [key("class field", "f")] = 9;
}
const c: any = new C();
console.log(c.m(), Object.keys(c).join());

// 23.1.3: index, count and depth operands.
console.log([1, 2, 3, 4].slice(num("slice")).join());
console.log([1, 2, 3, 4].splice(num("splice")).join());
console.log([1, 2, 1, 4].lastIndexOf(1, num("lastIndexOf")));
console.log([1, 2, 3, 4].copyWithin(0, num("copyWithin")).join());
console.log([[1, [2, [3]]]].flat(num("flat")).length);
console.log([1, 2, 3, 4].indexOf(3, num("indexOf")), [1, 2, 3, 4].includes(1, num("includes")));
console.log([1, 2, 3, 4].fill(0, num("fill")).join());

// 7.1.1 step 2: an @@toPrimitive that returns a primitive is the whole
// conversion; `toString` does not run.
const p: any = {
  [Symbol.toPrimitive](hint: string) {
    log.push("toPrimitive " + hint);
    return arg || "a";
  },
  toString() {
    log.push("toString (never)");
    return "x";
  },
};
const o3: any = { a: 1 };
delete o3[p];
// 7.1.1.1 hint string: Object.prototype.toString answers first; the own
// `valueOf` does not run.
const q: any = {
  valueOf() {
    log.push("valueOf (never)");
    return "a";
  },
};
const o4: any = { "[object Object]": 1, a: 2 };
delete o4[q];
console.log(Object.keys(o3).length, Object.keys(o4).join());
console.log(log.join("\n"));
