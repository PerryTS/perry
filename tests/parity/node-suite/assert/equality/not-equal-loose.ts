import assert from "node:assert";

assert.notEqual(1, 2);
try { assert.notEqual(1, "1" as unknown as number); } catch (err) { console.log("not equal loose same:", (err as { operator?: string }).operator); }
console.log("not equal loose ok");
