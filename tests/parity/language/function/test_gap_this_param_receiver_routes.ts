import * as util from "node:util";
const out: string[] = [];
const obj: any = {
  k: 7,
  cb(this: any, x: number, done: (e: any, v?: any) => void) { done(null, this && this.k + x); },
  async am(this: any, x: number) { return this.k * x; },
  old(this: any) { return this && this.k; },
};
obj.p = util.promisify(obj.cb);
obj.c = util.callbackify(obj.am);
obj.d = util.deprecate(obj.old, "old is deprecated", "DEP_TEST");
function argsAccessor(this: any, a: number) {
  const args: any = arguments;
  Object.defineProperty(args, "g", { get(this: any) { return this === args; }, configurable: true });
  return args.g;
}
out.push("args-accessor-this=" + argsAccessor(1));
const holder: any = Object.create((Intl as any).NumberFormat.prototype);
const r = (Intl as any).NumberFormat.call(holder, "en-US");
out.push("intl-legacy-chain=" + (r === holder));
out.push("deprecate=" + obj.d());
const sorted = [3, 1, 2].sort(function (this: any, a: number, b: number) { return (this === undefined ? 0 : 100) + a - b; });
out.push("sort=" + sorted.join(""));
obj.p(3).then((v: any) => {
  out.push("promisify=" + v);
  obj.c(5, (e: any, v: any) => {
    out.push("callbackify=" + v);
    console.log(out.join(" "));
  });
});
