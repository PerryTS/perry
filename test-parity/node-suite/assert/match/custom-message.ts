import assert from "node:assert";

try { assert.match("abc", /z/, "must contain z"); } catch (err) {
  const e = err as { message: string; generatedMessage?: boolean; code?: string };
  console.log("match:", e.message === "must contain z", e.generatedMessage, e.code);
}
try { assert.doesNotMatch("abc", /b/, "must not contain b"); } catch (err) {
  const e = err as { message: string; generatedMessage?: boolean; code?: string };
  console.log("doesNotMatch:", e.message === "must not contain b", e.generatedMessage, e.code);
}
