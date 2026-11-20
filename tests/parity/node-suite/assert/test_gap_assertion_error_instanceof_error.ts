// An AssertionError is still an Error when its prototype chain is not materialized.
import assert from "node:assert";

try {
  assert.strictEqual(1, 2);
} catch (e) {
  console.log("thrown instanceof Error:", e instanceof Error);
}
const made = new assert.AssertionError({ message: "x" });
console.log("constructed instanceof Error:", made instanceof Error);
console.log("plain object instanceof Error:", {} instanceof Error);
console.log("Object.create(Error.prototype) instanceof Error:", Object.create(Error.prototype) instanceof Error);
