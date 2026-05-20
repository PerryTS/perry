import assert from "node:assert";

assert.deepEqual(1, 1);
assert.notDeepEqual(1, 2);
try { assert.notDeepEqual("same", "same"); } catch (err) { console.log("not deep equal same:", (err as { operator?: string }).operator); }
console.log("legacy deep aliases ok");
