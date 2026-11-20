import assert from "node:assert/strict";
import { equal, notEqual } from "node:assert/strict";

console.log("strict default:", typeof assert.strictEqual);
try { equal(1, "1" as unknown as number); } catch (err) { console.log("equal strict throws:", (err as Error).name); }
notEqual(1, 2);
console.log("strict submodule ok");
