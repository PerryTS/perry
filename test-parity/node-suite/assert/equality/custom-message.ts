import assert from "node:assert";

try { assert.strictEqual(1, 2, "numbers differ"); } catch (err) { console.log("strict message:", err.message.split("\n")[0]); }
try { assert.notEqual(1, "1", "loose same"); } catch (err) { console.log("notEqual message:", err.message); }
