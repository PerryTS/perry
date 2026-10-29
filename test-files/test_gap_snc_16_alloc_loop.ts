// SNC 16 (GC stress subject): every fused await carries a fresh heap value; identity and
// contents must survive moving collections between the in-place resumes.
async function chain(n: number): Promise<number> {
  let sum = 0;
  let keep: any[] = [];
  for (let i = 0; i < n; i++) {
    const obj = { id: i, tag: "t" + i, items: [i, i + 1, i + 2], cb: (x: number) => x + i };
    const got = await Promise.resolve(obj);
    if (got !== obj || got.tag !== "t" + i || got.items[2] !== i + 2 || got.cb(1) !== i + 1) throw new Error("lost object " + i);
    const viaPrim: number = await (i * 2);
    sum += viaPrim + got.items.length;
    if (i % 997 === 0) keep.push(got);
    if (keep.length > 8) keep = keep.slice(4);
  }
  return sum + keep.length;
}
chain(30000).then((s) => console.log("sum", s));
