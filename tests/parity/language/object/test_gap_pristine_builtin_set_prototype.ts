// #11915: [[Prototype]] changes on a builtin prototype and on a receiver.
// DEC 250 locks Map.prototype's [[Prototype]]: the receiver adopts a user map
// prototype MP (below Map.prototype) whose link changes instead.
const MP = Object.create(Map.prototype);
const m = Object.setPrototypeOf(new Map<string, number>([['k', 1]]), MP);
function run(x: any) { console.log(x.get('k'), typeof x.extra === 'function' ? x.extra() : 'none'); }
run(m);
const extraProto = Object.create(Map.prototype);
extraProto.extra = function () { return 'extra'; };
Object.setPrototypeOf(MP, extraProto);
run(m);
const a = [1, 2, 3];
function inc(x: number[]) { return x.includes(2); }
console.log(inc(a));
const fake = { includes() { return 'fake-includes'; } };
Object.setPrototypeOf(a, fake);
console.log(inc(a));
