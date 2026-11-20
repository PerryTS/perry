// Template substitutions are ToString, not `+`'s ToPrimitive: a substitution
// whose coercion the concat helper performs itself must still print exactly
// what String(x) prints, and a substitution whose coercion is observable must
// still run it. Covers the signs, zeroes, non-finites, integer/fraction split,
// the scientific-notation thresholds and the tie-break ryu-js owns (#3987),
// BigInt, and objects with valueOf/toString.

function tpl(s: string, n: number): string {
  return `${s}:${n}`;
}
function tplAny(a: any, b: any): string {
  return `${a}|${b}`;
}

const label = ["v", "al"].join("");

// Signs and zeroes: -0 prints as "0" in a template, unlike Object.is.
console.log(tpl(label, 0), tpl(label, -0), tpl(label, 1), tpl(label, -1));
console.log("neg-zero", `${-0}`, `${0}`, Object.is(-0, -0 * 1));

// Non-finites.
console.log(tpl(label, NaN), tpl(label, Infinity), tpl(label, -Infinity));

// Integers, including the 2^53 boundary and negatives.
console.log(tpl(label, 42), tpl(label, -42), tpl(label, 1e15), tpl(label, 2 ** 53));
console.log(tpl(label, Number.MAX_SAFE_INTEGER), tpl(label, -Number.MAX_SAFE_INTEGER));

// Fractions and the shortest-round-trip tie-break.
console.log(tpl(label, 0.1), tpl(label, 1 / 3), tpl(label, 0.5), tpl(label, 1.005));
console.log(tpl(label, 5e-324), tpl(label, Number.MAX_VALUE), tpl(label, Number.MIN_VALUE));

// The scientific-notation thresholds: >= 1e21 and < 1e-6 switch form.
console.log(tpl(label, 1e20), tpl(label, 1e21), tpl(label, 1e-6), tpl(label, 1e-7));
console.log(tpl(label, 123456789012345680000), tpl(label, 0.000001), tpl(label, 0.0000001));

// Every one of these must agree with String(x) and with `+`.
const values = [0, -0, 1, -1, NaN, Infinity, -Infinity, 0.1, 1e21, 1e-7, 2 ** 53, 5e-324];
console.log("agree", values.every((v) => `${v}` === String(v) && `${v}` === "" + v));

// A number reached through `any` (the annotation cannot be trusted).
console.log(tplAny(1.5, -2.5), tplAny(0, NaN), tplAny(1e21, 1e-7));

// A lying annotation: the parameter says number, the value is not.
console.log("lie", tpl(label, "12" as any), tpl(label, true as any), tpl(label, null as any));
console.log("lie-obj", tpl(label, { toString: () => "OBJ" } as any));

// ToString is toString-first, unlike `+` which is valueOf-first.
const both = {
  valueOf() {
    return 111;
  },
  toString() {
    return "STR";
  },
};
console.log("tostring-first", `${both}`, String(both), "" + both, tplAny(both, both));

// A substitution whose coercion has a side effect must run exactly once.
let calls = 0;
const counted = {
  toString() {
    calls++;
    return "C";
  },
};
const once = `${counted}-${counted}`;
console.log("side-effect", once, calls);

// Symbol.toPrimitive wins over both.
const prim = {
  [Symbol.toPrimitive](hint: string) {
    return "P:" + hint;
  },
};
console.log("toPrimitive", `${prim}`, "" + (prim as any));

// BigInt substitutions.
console.log("bigint", `${10n}`, `${-10n}`, `${2n ** 64n}`, tplAny(1n, 2n));

// Strings, booleans, null/undefined and arrays keep their forms.
console.log("mixed", tplAny("s", true), tplAny(null, undefined), tplAny([1, 2], {}));

// Longer chains and nesting.
const n1 = 1.25;
console.log(`a${n1}b${-n1}c${n1 * 4}d`, `${`${n1}`}`);

// A hot loop, the shape the in-place formatting is for.
let acc = "";
for (let i = 0; i < 200; i++) acc = `${i}:${i / 8}`;
console.log("hot", acc, acc.length);
