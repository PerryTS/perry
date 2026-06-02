import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.name, err?.operator || err?.code || "no-code"); }
}
show("same promise", () => { const p = Promise.resolve(1); assert.deepStrictEqual(p, p); });
show("different promise", () => assert.deepStrictEqual(Promise.resolve(1), Promise.resolve(1)));
show("weakmap identity", () => { const w = new WeakMap(); assert.deepStrictEqual(w, w); });
show("weakmap different", () => assert.deepStrictEqual(new WeakMap(), new WeakMap()));
