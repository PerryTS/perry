import assert from "node:assert";

try { assert.ok(false, "custom failure"); } catch (err) { console.log("message:", (err as Error).message); }
try { assert.fail("explicit failure"); } catch (err) { console.log("fail:", (err as Error).name, (err as Error).message); }
