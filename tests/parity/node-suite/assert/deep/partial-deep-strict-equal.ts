import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.name, err?.operator || err?.code || "no-code"); }
}
show("object subset", () => assert.partialDeepStrictEqual({ a: 1, b: 2 }, { a: 1 }));
show("nested subset", () => assert.partialDeepStrictEqual({ a: { b: 2, c: 3 } }, { a: { b: 2 } }));
show("array subset", () => assert.partialDeepStrictEqual([1, 2, 3], [1, 2]));
show("mismatch", () => assert.partialDeepStrictEqual({ a: 1 }, { a: 2 }));
