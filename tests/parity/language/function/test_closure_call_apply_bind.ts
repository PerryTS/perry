// call, apply and bind on closures: receiver and arguments, a bound function's name and
// length, binding a bound function, new on a bound function, a bound callback, and an
// async body. Closures are instances of Function and convert to a string.
const add = function (this: any, a: number, b: number): number { return (this ? this.k : 0) + a + b; };
function plain(a: number, b: number, c: number) { return a + b + c; }
const arrow = (x: number, y: number) => x * y;
const o = { k: 100 };
console.log(typeof add.bind, typeof add.call, typeof add.apply, add instanceof Function, Object.create(add) instanceof Function);
console.log(add.call(o, 1, 2), add.apply(o, [3, 4]), arrow.call(null, 3, 4), arrow.apply(null, [5, 6]), plain.apply(null, [1, 2, 3]));
const b = add.bind(o, 1);
console.log(typeof b, b(2), b.name, b.length, plain.bind(null, 1).name, plain.bind(null, 1)(2, 3), arrow.bind(null, 1, 2, 3).length);
const bb = b.bind(null, 5);
console.log(bb(), bb.name, bb.length);
const Pt = function (this: any, x: number, y: number) { this.x = x; this.y = y; };
const BP: any = Pt.bind(null, 7);
const p = new BP(8);
console.log(p.x, p.y);
console.log([1, 2, 3].map(plain.bind(null, 10, 20)).join(","), typeof String(add), (arrow + "!").endsWith("!"));
async function inAsync(): Promise<void> {
  const c = add.bind(o, 10);
  console.log("async", c(5), add.call(o, 1, 1), add.apply(o, [2, 2]), c.name, c.length);
}
inAsync();
