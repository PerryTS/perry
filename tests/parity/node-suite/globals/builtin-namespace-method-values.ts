function show(label: string, value: any) {
  console.log(label, "typeof", typeof value);
  console.log(label, "names", Object.getOwnPropertyNames(value).join("|"));
  console.log(
    label,
    "own",
    Object.prototype.hasOwnProperty.call(value, "length"),
    Object.prototype.hasOwnProperty.call(value, "name"),
  );
  console.log(
    label,
    "meta",
    typeof value === "function" ? value.name : "<not-function>",
    typeof value === "function" ? value.length : "<not-function>",
  );
}

const mathAbs = Math.abs;
const objectKeys = Object.keys;
const jsonStringify = JSON.stringify;
const reflectApply = Reflect.apply;
const bigintAsIntN = BigInt.asIntN;
const symbolFor = Symbol.for;

show("Math.abs", mathAbs);
show("Object.keys", objectKeys);
show("JSON.stringify", jsonStringify);
show("Reflect.apply", reflectApply);
show("BigInt.asIntN", bigintAsIntN);
show("Symbol.for", symbolFor);

console.log(
  "Math.abs call",
  typeof mathAbs === "function" ? mathAbs(-5) : "<not-function>",
);
console.log(
  "Object.keys call",
  typeof objectKeys === "function" ? objectKeys({ b: 2, a: 1 }).join("|") : "<not-function>",
);
console.log(
  "JSON.stringify call",
  typeof jsonStringify === "function" ? jsonStringify({ a: 1 }) : "<not-function>",
);
console.log(
  "Reflect.apply call",
  typeof reflectApply === "function" ? reflectApply(Math.max, null, [3, 7]) : "<not-function>",
);
console.log(
  "BigInt.asIntN call",
  typeof bigintAsIntN === "function" ? bigintAsIntN(4, 31n).toString() : "<not-function>",
);
console.log(
  "Symbol.for call",
  typeof symbolFor === "function" ? String(symbolFor("perry.issue4437")) : "<not-function>",
);
