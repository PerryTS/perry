import assert from "node:assert";

assert.match("hello world", /world/);
assert.match("HELLO", /hello/i);
try { assert.match("hello", /world/); } catch (err) { console.log("match miss:", (err as { operator?: string }).operator); }
console.log("match basic ok");
