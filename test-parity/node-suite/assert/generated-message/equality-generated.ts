import assert from "node:assert";

try { assert.equal(1, 2); } catch (err) {
  const e = err as { generatedMessage?: boolean; operator?: string; code?: string };
  console.log("equal generated:", e.generatedMessage, e.operator, e.code);
}
try { assert.strictEqual(1, 2, "manual"); } catch (err) {
  const e = err as { generatedMessage?: boolean; operator?: string; code?: string; message: string };
  console.log("strict manual:", e.generatedMessage, e.operator, e.code, e.message.startsWith("manual"));
}
