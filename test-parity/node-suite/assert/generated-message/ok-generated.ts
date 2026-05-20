import assert from "node:assert";

try { assert.ok(false); } catch (err) {
  const e = err as { generatedMessage?: boolean; operator?: string; code?: string };
  console.log("ok generated:", e.generatedMessage, e.operator, e.code);
}
try { assert.ok(false, "manual"); } catch (err) {
  const e = err as { generatedMessage?: boolean; operator?: string; code?: string; message: string };
  console.log("ok manual:", e.generatedMessage, e.operator, e.code, e.message === "manual");
}
