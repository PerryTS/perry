// SNC 10: patching Promise.prototype.then / an instance / constructor mid-loop.
const log: string[] = [];
// DEC 250 locks Promise.prototype: the awaited promises adopt a user promise
// prototype UPP (below Promise.prototype), and `then` is patched there.
const UPP: any = Object.create(Promise.prototype);
function res<T>(v: T): Promise<T> { return Object.setPrototypeOf(Promise.resolve(v), UPP); }
async function loop(): Promise<void> {
  for (let i = 0; i < 9; i++) {
    if (i === 3) {
      const orig = Promise.prototype.then;
      UPP.then = function (this: any, ...a: any[]) {
        log.push("then-called");
        return orig.apply(this, a as any);
      };
    }
    if (i === 6) {
      const p = res(60);
      Object.defineProperty(p, "then", { get() { log.push("own-then-get"); return Promise.prototype.then; } });
      log.push("v" + (await p));
    }
    const v = await res(i);
    log.push("a" + v);
  }
}
loop().then(() => {
  log.push("end");
  console.log(log.join(" "));
});
