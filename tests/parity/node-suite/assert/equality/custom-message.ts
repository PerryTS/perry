import assert from "node:assert";

try { assert.strictEqual(1, 2, "numbers differ"); } catch (err) {
  const e = err as { message: string; generatedMessage?: boolean; code?: string };
  console.log("strict:", e.message.startsWith("numbers differ"), e.generatedMessage, e.code);
}
try { assert.notEqual(1, "1", "loose same"); } catch (err) {
  const e = err as { message: string; generatedMessage?: boolean; code?: string };
  console.log("notEqual:", e.message === "loose same", e.generatedMessage, e.code);
}
