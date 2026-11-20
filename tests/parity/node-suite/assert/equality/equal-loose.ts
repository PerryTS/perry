import assert from "node:assert";

assert.equal(1, "1" as unknown as number);
assert.equal(false, 0 as unknown as boolean);
try { assert.equal(1, 2); } catch (err) { console.log("equal mismatch:", (err as { operator?: string }).operator); }
console.log("loose equal ok");
