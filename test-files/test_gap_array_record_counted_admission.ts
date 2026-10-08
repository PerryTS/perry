// Counted (packed-f64) for-of copies are admitted only by the entry proof
// plus the live layout; a static type or an erased source never decides it.
function sumTyped(a: number[]): any {
  let total: any = 0;
  for (const x of a) total += x;
  return total;
}
function sumAny(a: any): any {
  let total: any = 0;
  for (const x of a) total += x;
  return total;
}
const numbers: number[] = [1, 2.5, 3];
console.log("typed numbers", sumTyped(numbers), sumAny(numbers));
const lying: number[] = [1, "x" as any, 3];
console.log("typed non-number layout", sumTyped(lying), sumAny(lying));
const frozen: number[] = Object.freeze([4, 5, 6]) as number[];
console.log("frozen", sumTyped(frozen), sumAny(frozen));
const holes: number[] = [1, , 3];
console.log("holes", sumTyped(holes), sumAny(holes));
(Array.prototype as any)[1] = 100;
console.log("holes read through the prototype", sumTyped(holes), sumAny(holes));
delete (Array.prototype as any)[1];
const grown: number[] = [];
for (let i = 0; i < 40; i++) grown.push(i * 0.5);
console.log("grown", sumTyped(grown), sumAny(grown));
const own: number[] = [7, 8];
(own as any)[Symbol.iterator] = function () {
  let i = 0;
  return { next: () => (i++ < 3 ? { value: "o", done: false } : { value: undefined, done: true }) };
};
console.log("own typed", sumTyped(own), "own erased", sumAny(own));
const proto: any = Array.prototype;
const values = proto[Symbol.iterator];
proto[Symbol.iterator] = function (this: number[]) {
  let i = 0;
  return { next: () => (i < this.length ? { value: this[i++] * 10, done: false } : { value: undefined, done: true }) };
};
console.log("prototype typed", sumTyped([1, 2]), "prototype erased", sumAny([1, 2]));
proto[Symbol.iterator] = values;
const itp: any = Object.getPrototypeOf(values.call([]));
const next = itp.next;
itp.next = function (this: any) {
  const r = next.call(this);
  if (!r.done) r.value = -r.value;
  return r;
};
console.log("next typed", sumTyped([1, 2]), "next erased", sumAny([1, 2]));
itp.next = next;
console.log("restored", sumTyped([1, 2]), sumAny([1, 2]));
