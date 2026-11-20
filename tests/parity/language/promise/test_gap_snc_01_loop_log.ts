// SNC 1: the 42-async shape with per-iteration logging.
async function chain(n: number): Promise<number> {
  let sum = 0;
  for (let i = 0; i < n; i++) {
    const v = await Promise.resolve(i);
    sum += v;
    if (i % 5 === 0) console.log("iter", i, sum);
  }
  return sum;
}
chain(23).then((s) => console.log("done", s));
console.log("sync-end");
