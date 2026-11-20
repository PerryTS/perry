// Values and printing: numbers, strings, template literals, objects, arrays.
const n = 42;
const s = "wasi";
console.log(n, n / 8, -0, 1e21, 0.1 + 0.2);
console.log(`hello ${s} ${n * 2}`);
console.log({ a: 1, b: "two", c: [1, 2, { d: null }] });
console.log([1, "x", true, undefined, null]);
console.log(typeof n, typeof s, typeof {}, typeof undefined, typeof (() => 0));
console.log(String(123).padStart(6, "0"), "a,b,c".split(","), "WASI".toLowerCase());
