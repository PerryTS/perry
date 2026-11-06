import assert from "node:assert";

async function main(): Promise<void> {
  await assert.rejects(async () => { throw new TypeError("async bad"); }, TypeError);
  console.log("rejects ctor: pass");
  await assert.rejects(Promise.reject(Object.assign(new Error("coded"), { code: "E_CODE" })), { code: "E_CODE" });
  console.log("rejects object: pass");
  await assert.doesNotReject(Promise.resolve("ok"));
  console.log("doesNotReject: pass");
  try { await assert.rejects(Promise.resolve("nope")); } catch (err: any) { console.log("rejects resolve:", err?.name, err?.code || err?.operator); }
}
await main();
