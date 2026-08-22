function boom() { throw "call-boom"; }
function syncCatch() {
  try { throw "sync-boom"; } catch (e: any) { console.log("sync caught:", e); }
}
async function noPriorAwait() {
  try { throw "no-await-boom"; } catch (e: any) { console.log("no-await caught:", e); }
}
async function directPostAwait() {
  await 0;
  try { throw "direct-boom"; } catch (e: any) { console.log("direct caught:", e); }
}
async function callPostAwait() {
  await 0;
  try { boom(); } catch (e: any) { console.log("call caught:", e); }
}
async function promiseResolvePostAwait() {
  await Promise.resolve(1);
  try { throw new Error("promise-boom"); } catch (e: any) { console.log("promise caught:", e.message); }
}
async function rejectedAwaitStillWorks() {
  try {
    await Promise.reject("reject-boom");
    console.log("reject unexpectedly resolved");
  } catch (e: any) { console.log("reject caught:", e); }
}

syncCatch();
await noPriorAwait();
await directPostAwait();
await callPostAwait();
await promiseResolvePostAwait();
await rejectedAwaitStillWorks();
console.log("done");
