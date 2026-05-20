import assert from "node:assert";

assert.doesNotMatch("hello", /world/);
try { assert.doesNotMatch("hello world", /world/); } catch (err) { console.log("does not match hit:", (err as { operator?: string }).operator); }
console.log("does not match ok");
