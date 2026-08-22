function sleep(ms: number): Promise<void> { return new Promise((resolve) => setTimeout(resolve, ms)); }
async function twoAwaitsThenThrow(): Promise<void> {
  await sleep(1);
  await sleep(1);
  throw new Error("boom");
}
async function callLayer(): Promise<void> { await twoAwaitsThenThrow(); }
async function main(): Promise<void> {
  try {
    await callLayer();
    console.log("no throw (wrong)");
  } catch (e: any) { console.log("caught " + e?.message); }
  console.log("done");
}
main();
