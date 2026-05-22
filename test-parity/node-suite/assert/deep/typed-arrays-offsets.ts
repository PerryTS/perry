import assert from "node:assert";

const ab1 = new ArrayBuffer(8);
const ab2 = new ArrayBuffer(8);
new Uint8Array(ab1).set([0, 1, 2, 3, 4, 5, 6, 7]);
new Uint8Array(ab2).set([9, 1, 2, 3, 4, 5, 6, 9]);
const a = new Uint8Array(ab1, 1, 6);
const b = new Uint8Array(ab2, 1, 6);
const c = new Int8Array(ab2, 1, 6);
try { assert.deepStrictEqual(a, b); console.log("same bytes offset equal"); } catch (err: any) { console.log("offset equal err:", err?.operator); }
try { assert.deepStrictEqual(a, c); console.log("different ctor no throw"); } catch (err: any) { console.log("different ctor:", err?.operator); }
