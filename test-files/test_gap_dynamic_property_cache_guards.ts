// Reuse each generic read site across shapes, prototypes and descriptors.
function readKey(value: any): any { return value.key; }
const a: any = {key: 1};
const b: any = {extra: 2, key: 3};
const c: any = {first: 4, second: 5, key: 6};
for (let i = 0; i < 20; i++) {
  if (readKey(a) !== 1 || readKey(b) !== 3 || readKey(c) !== 6) throw new Error("plain cache");
}
const inherited: any = Object.create({key: 99});
for (let i = 0; i < 5; i++) console.log(readKey(a), readKey(inherited), readKey(b));
let calls = 0;
Object.defineProperty(a, "key", {get() { calls++; return 10 + calls; }, configurable: true});
console.log(readKey(a), readKey(b), readKey(a), calls);
delete b.key;
console.log(readKey(b), readKey(inherited));
b.key = 31;
console.log(readKey(b));
const proxy: any = new Proxy(c, {get(target, key) { calls++; return Reflect.get(target, key); }});
console.log(readKey(proxy), readKey(c), calls);
function readLength(value: any): any { return value.length; }
const lengthObject: any = {length: 77};
console.log(readLength(lengthObject), readLength([1, 2]), readLength("hello"), readLength(lengthObject));
function readSize(value: any): any { return value.size; }
const sized: any = {size: 8};
console.log(readSize(sized), readSize(new Map([[1, 2]])), readSize(new Set([1, 2])), readSize(sized));
const wide: any = {};
for (let i = 0; i < 100; i++) wide["field" + i] = i;
wide.key = 42;
for (let i = 0; i < 20; i++) if (readKey(wide) !== 42) throw new Error("overflow cache");
delete wide.key;
console.log(readKey(wide));
wide.key = 43;
console.log(readKey(wide), readKey(inherited));
console.log("dynamic property cache guards ok");
