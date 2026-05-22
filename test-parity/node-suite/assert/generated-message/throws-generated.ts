import assert from "node:assert";

try { assert.throws(() => 1); } catch (err: any) { console.log("missing throw:", err.generatedMessage, err.operator, typeof err.message); }
try { assert.doesNotThrow(() => { throw new TypeError("bad"); }); } catch (err: any) { console.log("unexpected throw:", err.generatedMessage, err.operator, err.actual?.name); }
