import assert from "node:assert";

assert.strictEqual(null, null);
assert.strictEqual(undefined, undefined);
assert.notStrictEqual(null, undefined);
try { assert.strictEqual(null, undefined); } catch (err) { console.log("null undefined strict:", (err as { operator?: string }).operator); }
console.log("strict null undefined ok");
