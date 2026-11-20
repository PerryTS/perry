// Number-to-string on every concat/template/toString route. The runtime
// formats into a stack buffer instead of a temporary heap string; the output
// must stay byte-identical to Node for integers of every size and sign,
// fractions, exponent thresholds, -0, NaN, Infinity and int32-boxed values.

const values: number[] = [
  0, -0, 1, -1, 7, 42, -42, 999999999, 1000000000, -1000000000, 4294967295,
  4294967296, -2147483648, 2147483647, 123456789012345, -123456789012345,
  999999999999999, -999999999999999, 1e15, -1e15, 1e16, 1e20, 1e21, -1e21,
  1e-6, 1e-7, -1e-7, 0.1, 0.2, 0.1 + 0.2, 1 / 3, -2 / 3, 714286.2142857143,
  25774.002075195312, -28.848648071289062, 5e-324, 1.7976931348623157e308,
  2.220446049250313e-16, NaN, Infinity, -Infinity,
];

const prefix = ["id", "x"].join("-");
for (const n of values) {
  const viaTemplate = `${prefix}:${n}`;
  const viaPlus = prefix + ":" + n;
  const viaLeft = n + ":" + prefix;
  const viaString = String(n);
  const viaToString = n.toString();
  const chain = "[" + n + "|" + n * 2 + "|" + -n + "]";
  console.log(viaTemplate, viaPlus, viaLeft, viaString, viaToString, chain);
}

// Int32-tagged values reach the formatter from integer arithmetic and
// collection iteration.
const ints = new Set<number>();
for (let i = -5; i < 5; i++) ints.add((i * 1000003) | 0);
const parts: string[] = [];
for (const v of ints) parts.push(`${v}`, "" + v, v + "");
console.log(parts.join(","));

// Many parts in one template exercise the chain buffer sizes.
let acc = "";
for (let i = 0; i < 12; i++) {
  const f = i * 1.25 - 7;
  acc = `${acc}${i}:${f}:${-i * 1e9}:${f / 3};`;
}
console.log(acc);
console.log([1.5, -2, 3e21, -0, NaN].join("/"));
