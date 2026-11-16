// A builtin method captured before the program replaces it keeps naming the
// builtin: `orig.call(r, ...)` inside the replacement runs the original, it
// does not re-enter the replacement by name.
// DEC 250: Function.prototype.bind and Array.prototype.push are locked in a compiled program, so the replacements are refused and the calls run the builtins.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
const log: string[] = [];
const origBind = Function.prototype.bind;
locked(() => { (Function.prototype as any).bind = function (this: any, t: any) {
  log.push("bind");
  return origBind.call(this, t);
}; });
function g(this: any) { return this; }
const bound: any = g.bind(7);
locked(() => { (Function.prototype as any).bind = origBind; });
console.log(typeof bound, JSON.stringify(log));
const origPush = Array.prototype.push;
locked(() => { (Array.prototype as any).push = function (this: any[], ...xs: any[]) {
  origPush.call(log, "push:" + xs.join(","));
  return origPush.apply(this, xs);
}; });
const arr: number[] = [1];
arr.push(2);
Array.prototype.push.call(arr, 3);
locked(() => { (Array.prototype as any).push = origPush; });
console.log(arr.join(","), JSON.stringify(log));
