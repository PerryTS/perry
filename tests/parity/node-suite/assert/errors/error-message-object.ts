import assert from "node:assert";

try { assert.ok(false, new Error("custom error")); } catch (err) { console.log("ok error object:", (err as Error).name, (err as Error).message); }
try { assert.fail(new TypeError("typed fail")); } catch (err) { console.log("fail error object:", (err as Error).name, (err as Error).message); }
