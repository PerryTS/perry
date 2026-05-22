import assert from "node:assert";

const sym = Symbol("k");
const a: any = { x: 1 };
a[sym] = 2;
const b: any = { x: 1 };
b[sym] = 2;
const c: any = { x: 1 };
c[sym] = 3;
try { assert.deepStrictEqual(a, b); console.log("symbol props equal"); } catch (err: any) { console.log("symbol props equal err:", err?.operator); }
try { assert.deepStrictEqual(a, c); console.log("symbol props mismatch no throw"); } catch (err: any) { console.log("symbol props mismatch:", err?.operator); }
