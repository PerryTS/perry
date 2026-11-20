// `await x` on a native promise runs PromiseResolve(%Promise%, x): it reads
// x.constructor and, when that is %Promise%, reuses x and attaches the
// reactions directly. A `then` on a user prototype below Promise.prototype is
// never called. A subclass instance answers its own constructor, so it is
// resolved as a thenable and its `then` runs.
const log: string[] = [];
const UPP: any = Object.create(Promise.prototype);
UPP.then = function (this: any, ...a: any[]) {
  log.push("UPP.then");
  return Promise.prototype.then.apply(this, a as any);
};
function adopt<T>(v: T): Promise<T> { return Object.setPrototypeOf(Promise.resolve(v), UPP); }

class Sub<T> extends Promise<T> {
  then(...a: any[]): any {
    log.push("Sub.then");
    return super.then(...(a as [any, any]));
  }
}

async function main(): Promise<void> {
  const p = adopt(1);
  log.push("same " + (Promise.resolve(p) === p));
  log.push("v" + (await p));
  for (let i = 2; i < 5; i++) log.push("v" + (await adopt(i)));

  const q = adopt(7);
  Object.defineProperty(q, "then", { get() { log.push("own-then-get"); return Promise.prototype.then; } });
  log.push("v" + (await q));

  const s = new Sub<number>((r) => r(9));
  log.push("sub-same " + (Promise.resolve(s) === s));
  log.push("v" + (await s));
}
main().then(() => console.log(log.join(" ")));
