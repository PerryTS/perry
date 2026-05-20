import assert from "assert";
import strictAssert from "assert/strict";

assert.strictEqual(typeof assert.ok, "function");
strictAssert.strictEqual(1, 1);
console.log("prefixless:", typeof assert.ok, typeof strictAssert.strictEqual);
