import assert from "node:assert";

await assert.rejects(async () => { throw "plain"; }, /plain/);
console.log("rejects string regexp");
await assert.rejects(async () => { const e: any = new Error("boom"); e.errno = 5; throw e; }, { errno: 5 });
console.log("rejects errno object");
try { await assert.doesNotReject(async () => { throw new Error("nope"); }); } catch (err: any) { console.log("doesNotReject throw:", err?.name, err?.code || err?.operator); }
