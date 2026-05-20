import assert from "node:assert";

assert.strictEqual(1n, 1n);
console.log("bigint equal");
try { assert.strictEqual(1n, 1); } catch (err) {
  const e = err as { code?: string };
  console.log("bigint vs number:", e.code);
}
