import assert from "node:assert";

assert.ifError(null);
assert.ifError(undefined);
try { assert.ifError(new Error("nested")); } catch (err) { console.log("if error Error:", (err as Error).name, (err as { code?: string }).code); }
try { assert.ifError("problem"); } catch (err) { console.log("if error string:", (err as Error).name, (err as { code?: string }).code); }
