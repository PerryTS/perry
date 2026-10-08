// Function receivers at warmed, untyped method sites (#12227).
function callSite(f: any, x: any, n: number): any { return f.call(x, n); }
function applySite(f: any, x: any, args: any): any { return f.apply(x, args); }
function pushSite(f: any, n: number): any { return f.push(n); }
function warm(f: any, x: any): any {
  let result: any;
  for (let i = 0; i < 40; i++) result = callSite(f, x, i);
  return result;
}
function target(this: any, n: number) { return this.k + n; }
const receiver = { k: 10 };
console.log("warm", warm(target, receiver));

// A changed [[Prototype]] revokes the inherited proof immediately.
const originalProto = Object.getPrototypeOf(target);
Object.setPrototypeOf(target, {
  call(this: any, x: any, n: number) { return "proto:" + x.k + ":" + n; }
});
console.log("prototype", callSite(target, receiver, 7));
Object.setPrototypeOf(target, originalProto);
console.log("prototype-restored", callSite(target, receiver, 7));

class C {}
let errors = 0;
for (let i = 0; i < 40; i++) {
  try { callSite(C, receiver, i); }
  catch (e) { if (e instanceof TypeError) errors++; }
}
console.log("class-errors", errors);

async function af(this: any, n: number) { return this.k + n; }
function* gf(this: any, n: number) { yield this.k; yield n; }
let generated = "";
for (let i = 0; i < 40; i++) generated = [...callSite(gf, receiver, i)].join(",");
console.log("generator", generated);
const promise = warm(af, receiver);

let maximum = 0;
for (let i = 0; i < 40; i++) maximum = applySite(Math.max, null, [i, 3, 9]);
console.log("max", maximum);
const array: number[] = [];
for (let i = 0; i < 40; i++) callSite(Array.prototype.push, array, i);
console.log("push-call", array.length, array[39]);

const proxy = new Proxy(target, {
  apply(f: any, x: any, args: any[]) { return Reflect.apply(f, x, args) + 100; }
});
console.log("proxy", warm(proxy, receiver));

const fp: any = originalProto;
const callKey = ["ca", "ll"].join("");
const applyKey = ["app", "ly"].join("");
const oldCall = fp[callKey];
const oldApply = fp[applyKey];
for (let i = 0; i < 40; i++) applySite(target, receiver, [i]);
fp[callKey] = function(this: any, x: any, n: number) { return "call-patch:" + n; };
const patchedCall = callSite(target, receiver, 7);
fp[callKey] = oldCall;
fp[applyKey] = function(this: any, x: any, args: any[]) { return "apply-patch:" + args[0]; };
const patchedApply = applySite(target, receiver, [8]);
fp[applyKey] = oldApply;
console.log("patched", patchedCall, patchedApply);
console.log("restored", callSite(target, receiver, 7), applySite(target, receiver, [8]));

function own() {}
const fn: any = own;
// Function length starts at zero; Array.prototype.push must use the ordinary
// array-like algorithm, including its throwing write of the read-only length.
fn.push = Array.prototype.push;
let ownErrors = 0;
for (let i = 0; i < 40; i++) {
  try { pushSite(fn, 1); }
  catch (e) { if (e instanceof TypeError) ownErrors++; }
}
console.log("own-push", ownErrors, fn[0], fn.length);
promise.then((n: number) => console.log("async", n));
