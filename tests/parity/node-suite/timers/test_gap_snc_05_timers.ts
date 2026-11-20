// SNC 5: timers/immediates pending during the loop fire only after it.
const log: string[] = [];
const late: string[] = [];
setTimeout(() => late.push("timeout0"), 0);
setImmediate(() => late.push("immediate"));
async function loop(): Promise<void> {
  for (let i = 0; i < 50; i++) {
    await i;
    if (i % 10 === 0) log.push("a" + i);
  }
  log.push("loop-end");
}
loop().then(() => setTimeout(() => console.log(log.join(" "), late.slice().sort().join(" ")), 5));
