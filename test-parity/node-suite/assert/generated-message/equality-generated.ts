import assert from "node:assert";

try { assert.equal(1, 2); } catch (err) { console.log("equal generated:", err.generatedMessage, err.operator); }
try { assert.strictEqual(1, 2, "manual"); } catch (err) { console.log("strict manual:", err.generatedMessage, err.message.split("\n")[0]); }
