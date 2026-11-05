// A builtin method captured before the program replaces it keeps naming the
// builtin: `orig.call(r, ...)` inside the replacement runs the original, it
// does not re-enter the replacement by name.
const log: string[] = [];
const origBind = Function.prototype.bind;
(Function.prototype as any).bind = function (this: any, t: any) {
  log.push("bind");
  return origBind.call(this, t);
};
function g(this: any) { return this; }
const bound: any = g.bind(7);
(Function.prototype as any).bind = origBind;
console.log(typeof bound, JSON.stringify(log));
const origPush = Array.prototype.push;
(Array.prototype as any).push = function (this: any[], ...xs: any[]) {
  origPush.call(log, "push:" + xs.join(","));
  return origPush.apply(this, xs);
};
const arr: number[] = [1];
arr.push(2);
Array.prototype.push.call(arr, 3);
(Array.prototype as any).push = origPush;
console.log(arr.join(","), JSON.stringify(log));
