// Deterministic JS compatibility checks for core runtime surface.
// Keep this file focused on behavior that can be compared byte-for-byte
// against Node with `run_parity_tests.sh`.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

// Math and numeric coercion.
line("math pow", Math.pow(2, 8));
line("math trunc", Math.trunc(-4.75));
line("math hypot", Math.hypot(3, 4));
line("math clz32", Math.clz32(1));
line("math imul", Math.imul(7, 6));
line("number finite", Number.isFinite(123.5));
line("number nan", Number.isNaN(NaN));
line("parse int", parseInt("101", 2));
line("parse float", parseFloat("3.50px"));

// Array methods with stable ordering.
const numbers = [1, 2, 3, 4, 5];
line("array map", numbers.map((n) => n * 2).join(","));
line("array filter", numbers.filter((n) => n % 2 === 1).join(","));
line("array reduce", numbers.reduce((a, b) => a + b, 0));
line("array includes", numbers.includes(3));
line("array find", numbers.find((n) => n > 3));
line("array some", numbers.some((n) => n === 4));
line("array every", numbers.every((n) => n > 0));
line("array flat", [1, [2, 3], [4]].flat().join(","));

// Object, symbols, and descriptor basics.
const sym = Symbol.for("perry.compat");
const obj: Record<string | symbol, unknown> = { a: 1 };
obj[sym] = "symbol-value";
Object.defineProperty(obj, "hidden", {
  value: 9,
  enumerable: false,
  configurable: true,
});
line("object keys", Object.keys(obj).join(","));
line("object has own", Object.hasOwn(obj, "a"));
line("object hidden", Object.getOwnPropertyDescriptor(obj, "hidden")?.enumerable);
line("symbol keyFor", Symbol.keyFor(sym));
line("symbol count", Object.getOwnPropertySymbols(obj).length);

// String and JSON paths.
const text = " Perry TS ";
line("string trim", text.trim());
line("string includes", text.includes("TS"));
line("string replace", text.replace("TS", "Runtime").trim());
line("json stringify", JSON.stringify({ a: 1, b: [2, 3] }));
line("json parse", JSON.parse('{"ok":true,"n":4}').n);

// Map and Set iteration order.
const map = new Map<string, number>();
map.set("first", 1);
map.set("second", 2);
line("map entries", Array.from(map.entries()).map(([k, v]) => k + "=" + v).join("|"));
const set = new Set<number>([3, 1, 3, 2]);
line("set values", Array.from(set.values()).join(","));

console.log("compat-core-surface: ok");
