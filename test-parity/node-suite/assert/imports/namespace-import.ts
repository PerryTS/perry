import * as assert from "node:assert";

console.log("namespace ok:", typeof assert.ok);
assert.ok(1);
assert.strict.strictEqual("x", "x");
console.log("namespace called");
