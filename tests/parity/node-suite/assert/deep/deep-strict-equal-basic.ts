import assert from "node:assert";

assert.deepStrictEqual(1, 1);
assert.deepStrictEqual("x", "x");
try { assert.deepStrictEqual(1, 2); } catch (err) { console.log("deep strict mismatch:", (err as { operator?: string }).operator); }
console.log("deep strict basic ok");
