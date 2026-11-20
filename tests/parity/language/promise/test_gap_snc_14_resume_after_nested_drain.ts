// SNC 14: re-entrancy and nested async calls; a nested activation never borrows the outer result.
const log: string[] = [];
async function inner(tag: string, n: number): Promise<string> {
  for (let i = 0; i < n; i++) await i;
  return tag;
}
async function outer(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    const r = await inner("i" + i, i);
    log.push(r);
  }
  const all = await Promise.all([inner("p", 2), inner("q", 1), 3]);
  log.push(all.join("+"));
  for (let i = 0; i < 3; i++) await i;
  log.push("outer-end");
}
outer().then(() => console.log(log.join(" ")));
inner("side", 3).then((v) => log.push("side-" + v));
