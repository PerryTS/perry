import assert from "node:assert";

try { assert.match("abc", /z/, "must contain z"); } catch (err) { console.log("match message:", (err as Error).message, (err as { generatedMessage?: boolean }).generatedMessage); }
try { assert.doesNotMatch("abc", /b/, "must not contain b"); } catch (err) { console.log("doesNotMatch message:", (err as Error).message, (err as { generatedMessage?: boolean }).generatedMessage); }
