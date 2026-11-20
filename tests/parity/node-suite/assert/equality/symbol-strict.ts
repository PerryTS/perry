import assert from "node:assert";

const s = Symbol("x");
assert.strictEqual(s, s);
console.log("same symbol equal");
try { assert.strictEqual(Symbol("x"), Symbol("x")); } catch (err) {
  const e = err as { code?: string };
  console.log("distinct symbols:", e.code);
}
