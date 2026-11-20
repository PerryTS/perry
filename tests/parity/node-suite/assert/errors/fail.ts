import assert from "node:assert";

try { assert.fail(); } catch (err) { console.log("fail default:", (err as Error).name, (err as { code?: string }).code); }
try { assert.fail("custom fail"); } catch (err) { console.log("fail custom:", (err as Error).message); }
