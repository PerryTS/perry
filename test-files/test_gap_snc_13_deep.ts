// SNC 13: a long straight await chain; no stack growth (also run under ulimit -s 512).
async function deep(n: number): Promise<number> {
  let s = 0;
  for (let i = 0; i < n; i++) s += await i;
  return s;
}
deep(400000).then((s) => console.log("sum", s));
