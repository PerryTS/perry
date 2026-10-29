// SNC 4: process.nextTick inside the loop runs after the whole microtask drain.
const log: string[] = [];
async function loop(): Promise<void> {
  process.nextTick(() => log.push("t0"));
  for (let i = 0; i < 3; i++) {
    await 0;
    log.push("a" + i);
    if (i === 0) process.nextTick(() => log.push("t1"));
  }
  process.nextTick(() => log.push("t2"));
}
setTimeout(() => {
  loop().then(() => {
    log.push("end");
    setTimeout(() => console.log(log.join(" ")), 0);
  });
}, 0);
