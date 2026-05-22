import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); } catch (err: any) { console.log(label + ":", err?.name, err?.operator || err?.code || "no-code"); }
}
const sparse = Array(3); sparse[1] = "x";
const explicit = [undefined, "x", undefined];
const sparse2 = Array(3); sparse2[1] = "x";
show("sparse equal", () => assert.deepStrictEqual(sparse, sparse2));
show("sparse vs explicit", () => assert.deepStrictEqual(sparse, explicit));
show("notDeep sparse", () => assert.notDeepStrictEqual(sparse, explicit));
