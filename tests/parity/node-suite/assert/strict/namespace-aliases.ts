import assert from "node:assert";

assert.strict.equal(1, 1);
assert.strict.notEqual(1, 2);
assert.strict.deepEqual("x", "x");
assert.strict.notDeepEqual("x", "y");
console.log("strict namespace aliases ok");
