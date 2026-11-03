// SNC 10: patching Promise.prototype.then / an instance / constructor mid-loop.
const log: string[] = [];
async function loop(): Promise<void> {
  for (let i = 0; i < 9; i++) {
    if (i === 3) {
      const orig = Promise.prototype.then;
      (Promise.prototype as any).then = function (this: any, ...a: any[]) {
        log.push("then-called");
        return orig.apply(this, a as any);
      };
    }
    if (i === 6) {
      const p = Promise.resolve(60);
      Object.defineProperty(p, "then", { get() { log.push("own-then-get"); return Promise.prototype.then; } });
      log.push("v" + (await p));
    }
    const v = await Promise.resolve(i);
    log.push("a" + v);
  }
}
loop().then(() => {
  log.push("end");
  console.log(log.join(" "));
});
