import assert from "node:assert/strict";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.name, err?.code || err?.operator || "no-code"); }
}
show("strict throws regexp", () => assert.throws(() => { throw new Error("abc"); }, /abc/));
show("strict throws object validator", () => assert.throws(() => { const e: any = new Error("boom"); e.code = "E_X"; throw e; }, { name: "Error", code: "E_X" }));
show("strict throws non-matching", () => assert.throws(() => { throw new Error("nope"); }, /will-not-match/));
try { await assert.rejects(async () => { throw new Error("async abc"); }, /async/); console.log("strict rejects: pass"); } catch (err: any) { console.log("strict rejects:", err?.name, err?.code || err?.operator); }
