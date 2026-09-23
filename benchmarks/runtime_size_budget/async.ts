const sleep = (ms: number) => new Promise<void>(r => setTimeout(r, ms));
async function work(n: number): Promise<number> { await sleep(1); return n * n; }
async function main() {
  const r = await Promise.all([1, 2, 3].map(work));
  console.log("results", r);
}
main();
