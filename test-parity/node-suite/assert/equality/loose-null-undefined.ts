import assert from "node:assert";

assert.equal(null, undefined);
assert.equal(2, "2" as unknown as number);
assert.notEqual(true, false);
try { assert.notEqual(null, undefined); } catch (err) { console.log("null undefined notEqual:", (err as { operator?: string }).operator); }
console.log("loose null undefined ok");
