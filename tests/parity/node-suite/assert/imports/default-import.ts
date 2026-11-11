import assert from "node:assert";

console.log("ok type:", typeof assert.ok);
assert.ok(true);
assert.strictEqual(1 + 1, 2);
console.log("default called");
