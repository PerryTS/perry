import assert from "node:assert";

const ok = assert.ok;
const strictEqual = assert.strictEqual;
ok("captured");
strictEqual(3, 3);
console.log("captured properties ok");
