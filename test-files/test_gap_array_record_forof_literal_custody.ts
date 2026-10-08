let evaluations = "";
function value(n: number): any {
  evaluations += n;
  return new Proxy({ n }, {});
}
let plain = "";
for (const item of [value(1), value(2)]) plain += item.n;
console.log("plain", evaluations, plain);
const proto: any = Array.prototype;
const original = proto[Symbol.iterator];
proto[Symbol.iterator] = function(this: any[]) {
  let i = 0;
  return { next: () => i < this.length ? {value: this[i++].n * 10, done: false} : {done: true} };
};
let custom = "";
for (const item of [value(3), value(4)]) custom += item;
proto[Symbol.iterator] = original;
console.log("override", evaluations, custom);
const itp: any = Object.getPrototypeOf([][Symbol.iterator]());
const next = itp.next;
itp.next = function(this: any) {
  const result = next.call(this);
  if (!result.done) result.value *= 100;
  return result;
};
let patched = 0;
for (const item of [5, 6]) patched += item;
itp.next = next;
console.log("next", patched);
let closeReceiver: any;
for (const item of [7, 8, 9]) {
  itp.return = function(this: any) { closeReceiver = this; return {done: true}; };
  console.log("first", item);
  break;
}
delete itp.return;
console.log("remaining", closeReceiver.next().value, closeReceiver.next().value, closeReceiver.next().done);
let holes = "";
Object.defineProperty(proto, "1", {get() { return 12; }, configurable: true});
for (const item of [1, , 3]) holes += item + ",";
delete proto[1];
console.log("holes", holes);
let spread = "";
for (const item of [0, ...[1, 2]]) spread += item;
console.log("spread", spread);
let castTotal = 0;
for (const item of ([10, 20] as number[])) castTotal += item;
console.log("cast", castTotal);
