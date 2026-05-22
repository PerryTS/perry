import assert from "node:assert/strict";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.name, err?.code || err?.operator || "no-code"); }
}
show("strict throws regexp", () => assert.throws(() => { throw new Error("abc"); }, /abc/));
show("strict rejects regexp", () => { throw new Error("sync marker"); });
try { await assert.rejects(async () => { throw new Error("async abc"); }, /async/); console.log("strict rejects: pass"); } catch (err: any) { console.log("strict rejects:", err?.name, err?.code || err?.operator); }
