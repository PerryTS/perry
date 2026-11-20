import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.name, err?.code || err?.operator || "no-code"); }
}
show("throw string exact", () => assert.throws(() => { throw "boom"; }, /^boom$/));
show("throw number any", () => assert.throws(() => { throw 7; }));
show("doesNotThrow primitive", () => assert.doesNotThrow(() => "ok"));
show("doesNotThrow catches", () => assert.doesNotThrow(() => { throw "bad"; }));
