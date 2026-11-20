import assert from "node:assert";

assert.deepStrictEqual({ a: { b: 1 } }, { a: { b: 1 } });
console.log("equal ok");
try { assert.deepStrictEqual({ a: { b: 1 } }, { a: { b: 2 } }); } catch (err) {
  const e = err as { name?: string; code?: string; operator?: string };
  console.log("mismatch:", e.name, e.code, e.operator);
}
