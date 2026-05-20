import assert from "node:assert";

try { assert.ok(false); } catch (err) { console.log("ok generated:", (err as { generatedMessage?: boolean; operator?: string }).generatedMessage, (err as { operator?: string }).operator); }
try { assert.ok(false, "manual"); } catch (err) { console.log("ok manual:", (err as { generatedMessage?: boolean; operator?: string }).generatedMessage, (err as Error).message); }
