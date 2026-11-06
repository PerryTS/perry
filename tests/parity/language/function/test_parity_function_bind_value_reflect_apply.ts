// Function.prototype.bind must work when invoked as a value.
const bind = Function.prototype.bind;
const call = Function.prototype.call;
const indexOf = String.prototype.indexOf;
const boundCall: any = (Reflect as any).apply(bind, call, [indexOf]);
if (typeof boundCall !== "function") throw new Error("not a function: " + typeof boundCall);
if (boundCall("hello", "l") !== 2) throw new Error("uncurry: " + boundCall("hello", "l"));

function add(a: number, b: number) { return a + b; }
const g: any = (bind as any).apply(add, [null, 10]);
if (typeof g !== "function") throw new Error("bind.apply not fn: " + typeof g);
if (g(5) !== 15) throw new Error("bind.apply partial: " + g(5));
console.log("OK");
