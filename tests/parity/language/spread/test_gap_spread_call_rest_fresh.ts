// f(...arr) into a rest parameter: the rest array is always a fresh array,
// never the caller's source (ArgumentListEvaluation + CreateArrayFromList).
function pushRest(...rest: any[]) { rest.push(99); rest[0] = "w"; return rest.length; }
function same(...rest: any[]) { return rest; }
function lead(a: any, b: any, ...rest: any[]) { rest.push("x"); return [a, b, rest.length, rest.join("|")].join(","); }
function args1(a: any) { arguments[0] = "A"; return arguments.length + ":" + Array.prototype.join.call(arguments, "|"); }
function argsRest(...rest: any[]) { rest.push(5); return arguments.length + ":" + rest.length; }
function argsLead(a: any, ...rest: any[]) { return [arguments.length, a, rest.length, arguments[1], arguments === (rest as any)].join(","); }
const arr = [1, 2, 3];
console.log(pushRest(...arr), arr.join(","), arr.length);
console.log(same(...arr) === arr, same(...arr).join(","));
const r = same(...arr); r.push(4); console.log(arr.length, r.length);
console.log(lead(...arr), arr.join(","));
console.log(lead(0, ...arr), arr.join(","));
console.log(lead(...arr, 7), arr.join(","));
console.log(lead(...[1]), lead(...[]));
console.log(args1(...arr), arr.join(","));
console.log(argsRest(...arr), arr.join(","));
console.log(argsLead(...arr), argsLead(9, ...arr), arr.join(","));
const empty: any[] = [];
const e2 = same(...empty); e2.push(1); console.log(empty.length, e2.length, e2 === empty);
let s = 0;
for (let i = 0; i < 1000; i++) { const t = same(...arr); t[0] = i; s += arr[0]; }
console.log(s);
// The same rule through a closure, a class method and an untyped receiver.
function mk() { return (...rest: any[]) => { rest.push(1); return rest; }; }
const g = mk();
console.log(g(...arr) === arr, arr.length);
class K { m(...rest: any[]) { rest.push(2); return rest; } n(a: any, ...rest: any[]) { rest.push(3); return rest; } }
const k = new K();
console.log(k.m(...arr) === arr, k.n(...arr).length, arr.length);
const o: any = k;
console.log(o.m(...arr) === arr, o.n(0, ...arr).join(","), arr.length);
// `arguments` is an arguments object, never an Array, also beside a rest parameter.
function isArgs(a: any, ...rest: any[]) { return Array.isArray(arguments) + ":" + Object.prototype.toString.call(arguments); }
function isArgs2(a: any) { return Array.isArray(arguments) + ":" + Object.prototype.toString.call(arguments); }
console.log(isArgs(...arr), isArgs(1, 2), isArgs2(...arr), isArgs2(1));
