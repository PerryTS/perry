function collect(a: number[]): string {
  let s: any = "";
  for (const x of a) s += String(x) + ",";
  return s;
}
const own: number[] = [1,2,3];
(own as any)[Symbol.iterator] = function() {
  let i = 0; return { next: () => i++ < 2 ? { value: i * 10, done: false } : { done: true } };
};
console.log("own", collect(own));
const proto: any = Array.prototype;
const values = proto[Symbol.iterator];
proto[Symbol.iterator] = function(this: number[]) {
  let i = 0; return { next: () => i < this.length ? { value: this[i++] * 3, done: false } : { done: true } };
};
console.log("prototype", collect([2,4]));
proto[Symbol.iterator] = values;
const itp: any = Object.getPrototypeOf(values.call([0]));
const next = itp.next;
itp.next = function(this: any) { const r = next.call(this); if (!r.done) r.value += 20; return r; };
console.log("next", collect([1,2]));
itp.next = next;
const changing: number[] = [1,2];
let result = "";
for (const x of changing) {
  result += x + ",";
  if (x === 1) { changing.push(3); itp.next = () => ({done:true}); }
}
itp.next = next;
console.log("mutation", result);
const holey: number[] = [1,,3];
console.log("holes", collect(holey));
