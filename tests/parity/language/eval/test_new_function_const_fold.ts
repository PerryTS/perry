// #1679 (Phase 1 of #1677) — literal `new Function` / `Function(...)` bodies
// are compiled to native functions instead of refused. Output must match
// `node --experimental-strip-types` byte-for-byte.

// single-expression body, two params
const add = new Function("a", "b", "return a + b");
console.log("add:", add(2, 3));

// multi-arg param names (three params)
const sum3 = new Function("a", "b", "c", "return a + b + c");
console.log("sum3:", sum3(1, 2, 3));

// comma-joined param list — Node treats this identically to separate args
const sub = new Function("a, b", "return a - b");
console.log("sub:", sub(10, 4));

// no params
const five = new Function("return 5");
console.log("five:", five());

// multi-statement body referencing a global (Math)
const hyp = new Function("x", "y", "const s = x * x + y * y; return Math.sqrt(s)");
console.log("hyp:", hyp(3, 4));

// the call form (no `new`) is equivalent
const mul = Function("a", "b", "return a * b");
console.log("mul:", mul(6, 7));
