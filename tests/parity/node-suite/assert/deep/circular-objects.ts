import assert from "node:assert";

const a: any = { name: "a" };
a.self = a;
const b: any = { name: "a" };
b.self = b;
const c: any = { name: "a" };
c.self = { name: "a" };
try { assert.deepStrictEqual(a, b); console.log("circular equal"); } catch (err: any) { console.log("circular equal err:", err?.operator); }
try { assert.deepStrictEqual(a, c); console.log("circular mismatch no throw"); } catch (err: any) { console.log("circular mismatch:", err?.operator); }
