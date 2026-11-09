// A getter or setter whose function is a class, a builtin or an
// Object.prototype method runs as an ordinary call of that function
// (B.2.2.2-3): a class constructor throws a TypeError before it reads `this`,
// `Object()` returns a fresh object, an Object.prototype method runs on the
// receiver. Holders keep their other members.
class Marker {}
class Cell {
  witness = 1;
}
const a: any = { cell: new Cell() };
a.__defineGetter__("k", Marker);
try {
  console.log(a.k);
} catch (e: any) {
  console.log(e instanceof TypeError, e.message);
}
a.__defineSetter__("s", Marker);
try {
  a.s = 1;
} catch (e: any) {
  console.log(e instanceof TypeError, e.message);
}
console.log(a.cell.witness);

const b: any = { cell: new Cell() };
b.__defineGetter__("k", Object);
const r1 = b.k;
const r2 = b.k;
console.log(typeof r1, r1 === r2, r1 === b, Object.keys(r1).length);
r1.cell = 0;
console.log(b.cell.witness);

const c: any = { cell: new Cell() };
c.__defineGetter__("k", Object.prototype.toString);
console.log(c.k);
c.__defineGetter__("v", Object.prototype.valueOf);
console.log(c.v === c);
c.v.cell = { witness: 2 };
console.log(c.cell.witness);
const d: any = Object.create(c);
console.log(d.v === d, d.k);
