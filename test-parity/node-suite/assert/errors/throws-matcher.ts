import assert from "node:assert";

function check(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); }
  catch (err: any) { console.log(label + ":", err?.name, err?.code || err?.operator || "no-code"); }
}

check("throws regexp", () => assert.throws(() => { throw new TypeError("bad value"); }, /bad/));
check("throws ctor", () => assert.throws(() => { throw new RangeError("range"); }, RangeError));
check("throws object", () => assert.throws(() => { const e: any = new Error("boom"); e.code = "E_X"; throw e; }, { name: "Error", message: "boom", code: "E_X" }));
check("does-not-throw", () => assert.doesNotThrow(() => 42));
check("missing throw", () => assert.throws(() => 1, /unused/));
