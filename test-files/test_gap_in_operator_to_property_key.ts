// #6944: the `in` operator must run ToPropertyKey on its left operand for
// EVERY key type, not just numbers. Per spec, `RelationalExpression in
// ShiftExpression` is `ToPropertyKey(lval)`, so an object key has its
// `Symbol.toPrimitive` / `toString` / `valueOf` invoked — exactly once, and
// even when the property is absent (the coercion is observable). Perry only
// coerced number keys, so an object key was compared as a raw pointer and
// never matched.

const obj: any = { here: 1 };

// object key with toString
const k: any = { toString(): string { return "here"; } };
console.log("object toString key :", (k as any) in obj);
console.log("string key          :", "here" in obj);

// the coercion runs exactly once, even when the property is ABSENT
let calls = 0;
const absent: any = {
  toString(): string {
    calls++;
    return "nope";
  },
};
console.log("absent object key   :", (absent as any) in obj);
console.log("coercion call count :", calls);

// Symbol.toPrimitive takes priority over toString/valueOf
const symPrim: any = {
  [Symbol.toPrimitive](hint: string): string {
    return "via-" + hint;
  },
  toString(): string {
    return "via-toString";
  },
};
const obj2: any = { "via-string": 1, "via-toString": 2 };
console.log("Symbol.toPrimitive  :", (symPrim as any) in obj2);

// OrdinaryToPrimitive(string) tries toString first; a non-primitive toString
// result falls through to valueOf
const valOf: any = {
  toString(): any {
    return {};
  },
  valueOf(): number {
    return 42;
  },
};
const obj3: any = { 42: "answer" };
console.log("valueOf fallback    :", (valOf as any) in obj3);

// toString wins over valueOf (string hint order)
const both: any = {
  toString(): string {
    return "str";
  },
  valueOf(): number {
    return 99;
  },
};
const obj4: any = { str: 1, 99: 2 };
console.log("toString > valueOf  :", (both as any) in obj4);

// an object key coercing to a Symbol matches the symbol-keyed property
const sym = Symbol("s");
const symKey: any = {
  toString(): any {
    return sym;
  },
};
const withSym: any = { [sym]: 1 };
console.log("object -> symbol key:", (symKey as any) in withSym);

// non-string primitives are stringified too
const primitives: any = { true: 1, null: 2, undefined: 3, "1": 4 };
console.log("boolean key         :", (true as any) in primitives);
console.log("null key            :", (null as any) in primitives);
console.log("undefined key       :", (undefined as any) in primitives);
console.log("bigint key          :", (1n as any) in primitives);

// int32-boxed and f64 numeric keys agree on a plain object
const small = 1;
console.log("int32 numeric key   :", (small as any) in primitives);
console.log("f64 numeric key     :", 1.0 in primitives);

// a proxy receiver observes the COERCED key in its `has` trap
const seen: string[] = [];
const proxy = new Proxy({ here: 1 }, {
  has(target, prop) {
    seen.push(String(prop));
    return prop in target;
  },
});
console.log("proxy has trap      :", (k as any) in proxy);
console.log("proxy saw key       :", seen.join(","));
