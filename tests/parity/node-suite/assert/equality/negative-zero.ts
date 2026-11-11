import assert from "node:assert";

const negZero = 1 / -Infinity;
try { assert.strictEqual(0, negZero); } catch (err) {
  const e = err as { code?: string };
  console.log("0 !== -0:", e.code);
}
assert.notStrictEqual(0, negZero);
console.log("ok");
