// #9713: a dynamically dispatched `x["toString"]()` on a number reached the
// native-method tower's own formatter — a bare Rust `f64::to_string()` — instead
// of ECMA-262 NumberToString. That prints `inf` for Infinity and the full
// decimal expansion past the exponential thresholds, so the same value
// stringified four static ways and once dynamically disagreed inside one
// program. Boxed `new Number(x)` receivers took the same wrong arm.
//
// Every row prints all the renderings so a future divergence shows which path
// moved, not just that something changed. `toLocaleString` is deliberately
// absent: node applies locale grouping there (`1,000,000,000,000,000,000,000`,
// `\u221e`) and perry does not, which is a separate gap this fixture should not
// be entangled with.

function dynCall(x: any, m: string): any { return x[m](); }
function dynCall1(x: any, m: string, a: any): any { return x[m](a); }

const values: [string, number][] = [
  ["1e21", 1e21],
  ["1e20", 1e20],
  ["1e-6", 1e-6],
  ["1e-7", 1e-7],
  ["-2.5e-9", -2.5e-9],
  ["2.2e-308", 2.2e-308],
  ["1e-310", 1e-310],
  ["MAX_VALUE", Number.MAX_VALUE],
  ["MIN_VALUE", Number.MIN_VALUE],
  ["EPSILON", Number.EPSILON],
  ["Infinity", Infinity],
  ["-Infinity", -Infinity],
  ["NaN", NaN],
  ["-0", -0],
  ["0.1", 0.1],
  ["255", 255],
  ["2**53", 9007199254740992],
  ["2**58", 288230376151711744],
];

for (const [label, n] of values) {
  const parts = [
    "static=" + n.toString(),
    "String=" + String(n),
    "tpl=" + `${n}`,
    "concat=" + (n + ""),
    "dyn=" + dynCall(n, "toString"),
    "boxed=" + dynCall(new Number(n), "toString"),
    "boxedValueOf=" + String(dynCall(new Number(n), "valueOf")),
  ];
  console.log(label + " :: " + parts.join(" | "));
}

// An explicit radix must still reach the radix formatter, and an explicit
// `undefined` radix must behave like no argument at all.
console.log("radix16=" + dynCall1(255, "toString", 16));
console.log("radix2=" + dynCall1(5, "toString", 2));
// Radix values stay at or below 2^53: above it perry's non-power-of-two radix
// formatter emits exact digits where V8 emits the shortest round-trip form
// (`(1e21).toString(36)` → `5v1j4f4ds7c4ks` vs `5v1j4f4ds7c000`), statically as
// well as dynamically. That is a separate defect and not what this pins.
console.log("radix36=" + dynCall1(9007199254740992, "toString", 36));
console.log("radix7=" + dynCall1(255, "toString", 7));
console.log("radixUndef=" + dynCall1(1e21, "toString", undefined));
console.log("boxedRadix16=" + dynCall1(new Number(255), "toString", 16));

// Sibling numeric methods on the same dynamic route, so a shared regression in
// the tower's number handling is visible here too.
console.log("toFixed=" + dynCall1(3.14159, "toFixed", 2));
console.log("toPrecision=" + dynCall1(1234.5678, "toPrecision", 6));
console.log("toExponential=" + dynCall1(1e21, "toExponential", 3));
