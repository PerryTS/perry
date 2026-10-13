// #11915: [[Prototype]] changes on a builtin prototype and on a receiver.
const m = new Map<string, number>([['k', 1]]);
function run(x: any) { console.log(x.get('k'), typeof x.extra === 'function' ? x.extra() : 'none'); }
run(m);
const extraProto = Object.create(Object.prototype);
extraProto.extra = function () { return 'extra'; };
Object.setPrototypeOf(Map.prototype, extraProto);
run(m);
const a = [1, 2, 3];
function inc(x: number[]) { return x.includes(2); }
console.log(inc(a));
const fake = { includes() { return 'fake-includes'; } };
Object.setPrototypeOf(a, fake);
console.log(inc(a));
