// SNC 12: unhandled rejection created inside the loop; report timing.
const log: string[] = [];
process.on("unhandledRejection", (e: any) => log.push("unhandled " + e.message));
async function loop(): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await i;
    if (i === 2) Promise.reject(new Error("r" + i));
    if (i === 4) { const p = Promise.reject(new Error("late")); await 0; p.catch(() => log.push("caught-late")); }
  }
  log.push("loop-end");
}
loop().then(() => log.push("resolved"));
setTimeout(() => console.log(log.join(" ")), 10);
