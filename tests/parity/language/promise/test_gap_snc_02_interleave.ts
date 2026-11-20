// SNC 2: two concurrent await loops must interleave a0 b0 a1 b1 ...
const log: string[] = [];
async function loop(name: string, n: number): Promise<void> {
  for (let i = 0; i < n; i++) {
    await 0;
    log.push(name + i);
  }
}
const a = loop("a", 6);
const b = loop("b", 4);
Promise.all([a, b]).then(() => console.log(log.join(" ")));
