// parity-node-argv: --expose-gc
export {};
declare function gc(): void;
const one: any = { value: 13 };
const two: any = { value: 29 };
const a: any = Object.create(one);
const fields = ["value", "value"];
function staticRead(o: any) { return o.value; }
function read(o: any) {
  const computed = o[fields[0]];
  if (staticRead(o) !== computed) throw new Error("holder and generic Get disagree");
  return computed;
}
const kind = Symbol("value");
a[kind] = 43;
console.log("key kinds", read(a), a[kind]);
a.value = 17;
console.log("shadow", read(a));
delete a.value;
console.log("unshadow", read(a));
for (let i = 0; i < 100; i++) read(a);
console.log(read(a));
Object.setPrototypeOf(a, two);
console.log(read(a));
delete two.value;
console.log(read(a));
two.value = 31;
console.log(read(a));
Object.defineProperty(two, "value", { configurable: true, get() {
  return this === a ? 37 : -1;
}});
console.log(read(a));
Object.setPrototypeOf(a, new Proxy(one, { get(_target, name, receiver) {
  return name === "value" && receiver === a ? 41 : undefined;
}}));
console.log(read(a));
Object.setPrototypeOf(a, null);
console.log(read(a), Object.getPrototypeOf(a) === null);
const bornNull: any = Object.create(null);
console.log(read(bornNull), Object.getPrototypeOf(bornNull) === null);
Object.setPrototypeOf(a, one);
console.log(read(a));
const dead: any[] = [];
function unrootedPair() {
  const proto = { marker: 47 };
  const receiver = Object.create(proto);
  // Never read through a site: this isolates the shape's prototype custody.
  dead.push(new WeakRef(proto), new WeakRef(receiver));
}
unrootedPair();
setImmediate(() => {
  gc(); gc(); gc();
  console.log("unrooted prototype", dead[0].deref() === undefined);
  console.log("unrooted receiver", dead[1].deref() === undefined);
  console.log("live prototype", read(a));
});
