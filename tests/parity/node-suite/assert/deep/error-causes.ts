import assert from "node:assert";

const a: any = new Error("outer", { cause: new TypeError("inner") });
const b: any = new Error("outer", { cause: new TypeError("inner") });
const c: any = new Error("outer", { cause: new RangeError("inner") });

try { assert.deepStrictEqual(a, b); console.log("cause equal"); } catch (err: any) { console.log("cause equal err:", err?.operator); }
try { assert.deepStrictEqual(a, c); console.log("cause mismatch no throw"); } catch (err: any) { console.log("cause mismatch:", err?.operator); }

const agg1: any = new AggregateError([new Error("a"), "b"], "agg");
const agg2: any = new AggregateError([new Error("a"), "b"], "agg");
try { assert.deepStrictEqual(agg1, agg2); console.log("aggregate equal"); } catch (err: any) { console.log("aggregate err:", err?.operator); }
