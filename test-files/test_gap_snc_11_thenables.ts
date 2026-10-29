// SNC 11: thenables (getter call counts and ordering) between fused awaits.
const log: string[] = [];
async function loop(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await i;
    log.push("p" + i);
  }
  const t = {
    get then() {
      log.push("get-then");
      return (res: (v: number) => void) => { log.push("call-then"); res(5); };
    },
  };
  log.push("t" + (await t));
  for (let i = 0; i < 4; i++) {
    await i;
    log.push("q" + i);
  }
  log.push("t2" + (await { then(res: (v: number) => void) { log.push("plain-then"); res(6); } } as any));
}
loop().then(() => console.log(log.join(" ")));
Promise.resolve().then(() => log.push("x1")).then(() => log.push("x2")).then(() => log.push("x3")).then(() => log.push("x4"));
