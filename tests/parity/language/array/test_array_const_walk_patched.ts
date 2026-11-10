// Fact IM, refused control: a program that replaces an iteration member.
// `for (const x of A)` over an immutable Array of Number literals is unrolled
// only when no program write can reach `Array.prototype[Symbol.iterator]`
// or `%ArrayIteratorPrototype%.next`. Here both are replaced, so every
// for-of must run the replacement, before and after it is installed.
const out: string[] = [];

const A = [1, 2, 3, 4];
const B = [10, 20];

function walk(xs: number[]): string {
  let s = "";
  for (const v of xs) s += v + ",";
  return s;
}
let direct = "";
for (const v of A) direct += v + ",";
out.push("before " + direct + walk(A) + walk(B));

const original = Array.prototype[Symbol.iterator];
(Array.prototype as any)[Symbol.iterator] = function* (this: number[]) {
  for (let i = this.length - 1; i >= 0; i--) yield this[i];
};
direct = "";
for (const v of A) direct += v + ",";
out.push("reversed " + direct + walk(A) + walk(B));
(Array.prototype as any)[Symbol.iterator] = original;

direct = "";
for (const v of A) direct += v + ",";
out.push("restored " + direct + walk(A) + walk(B));

const proto = Object.getPrototypeOf([][Symbol.iterator]());
const next = proto.next;
let calls = 0;
proto.next = function (this: unknown) {
  calls++;
  return next.call(this);
};
direct = "";
for (const v of A) direct += v + ",";
out.push("counted " + direct + walk(B) + calls);
proto.next = next;

// An own iterator on one Array.
const C = [5, 6, 7];
(C as any)[Symbol.iterator] = function* () {
  yield 99;
};
direct = "";
for (const v of C) direct += v + ",";
out.push("own " + direct);

console.log(out.join("\n"));
