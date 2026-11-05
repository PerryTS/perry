import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.name, err?.operator || err?.code || "no-code"); }
}
show("map equal", () => assert.deepStrictEqual(new Map([[{ a: 1 }, new Set([1, 2])]]), new Map([[{ a: 1 }, new Set([1, 2])]])));
show("set equal unordered", () => assert.deepStrictEqual(new Set([2, 1]), new Set([1, 2])));
show("map mismatch", () => assert.deepStrictEqual(new Map([["a", 1]]), new Map([["a", 2]])));
