import assert from "node:assert";

assert.deepStrictEqual(true, true);
assert.deepStrictEqual(null, null);
try { assert.deepStrictEqual(true, false); } catch (err) { console.log("boolean differs:", (err as { operator?: string }).operator); }
console.log("primitive deep ok");
