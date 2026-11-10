import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.operator || err?.name); }
}

show("uint8 equal", () => assert.deepStrictEqual(new Uint8Array([1, 2, 255]), new Uint8Array([1, 2, 255])));
show("uint8 mismatch", () => assert.deepStrictEqual(new Uint8Array([1, 2, 3]), new Uint8Array([1, 9, 3])));
show("different ctor", () => assert.deepStrictEqual(new Uint8Array([1, 2]), new Int8Array([1, 2])));
