import assert from "node:assert";

assert.deepStrictEqual(new Date(0), new Date(0));
console.log("dates equal");
assert.deepStrictEqual(/a/g, /a/g);
console.log("regex equal");
try { assert.deepStrictEqual(new Date(0), new Date(1)); } catch (err) {
  const e = err as { name?: string; code?: string };
  console.log("date mismatch:", e.name, e.code);
}
