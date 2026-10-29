// SNC 3: .then / queueMicrotask registered before and during the loop.
const log: string[] = [];
async function loop(): Promise<void> {
  Promise.resolve().then(() => log.push("then-before"));
  for (let i = 0; i < 8; i++) {
    if (i === 3) queueMicrotask(() => log.push("qm3"));
    if (i === 5) Promise.resolve().then(() => log.push("then5")).then(() => log.push("then5b"));
    await i;
    log.push("a" + i);
  }
}
loop().then(() => {
  log.push("end");
  console.log(log.join(" "));
});
