// Function objects carry a real ShapeId: bind/call/apply are answered by the
// Function.prototype slot the base shape names, and every way a function
// stops being "intrinsic-only" (own props, symbol keys, deletes, an own
// `call`) must still behave exactly as before.
function add(this: any, a: number, b: number, c: number): number {
  return (this && this.base ? this.base : 0) + a + b + c;
}
const b1 = add.bind({ base: 100 }, 1);
console.log("bind", b1(2, 3), b1.length, b1.name);
console.log("call", add.call({ base: 10 }, 1, 2, 3), add.call(null, 1, 2, 3));
console.log("apply", add.apply({ base: 20 }, [1, 2, 3]), add.apply(undefined, [4, 5, 6]));
const b2 = b1.bind(null, 2);
console.log("bind-of-bind", b2(3), b2.length, b2.name);

async function asyncFn(x: number) {
  return x * 2;
}
const ab = asyncFn.bind(null, 21);
ab().then((v) => console.log("async bind", v, Object.getPrototypeOf(asyncFn) === Object.getPrototypeOf(async function () {})));

function* gen(n: number) {
  for (let i = 0; i < n; i++) yield i;
}
console.log("generator call", [...gen.call(null, 3)].join(","));

// Own properties, symbol keys and deletes (the receiver leaves its base shape).
function withProps() {
  return 1;
}
(withProps as any).tag = "t";
(withProps as any)[Symbol.for("k")] = "sym";
console.log("props", (withProps as any).tag, (withProps as any)[Symbol.for("k")], withProps.call(null));
function withDelete(a: number, b: number) {
  return a + b;
}
delete (withDelete as any).length;
console.log("deleted length", withDelete.call(null, 1, 2), withDelete.apply(null, [3, 4]), withDelete.bind(null, 1)(5));
// A function whose own `call` shadows the prototype's.
function shadowed() {
  return 4;
}
(shadowed as any).call = () => "own call";
console.log("own call", (shadowed as any).call());
// Many binds (the Zod constructor pattern).
class Schema {
  parse(v: number) {
    return v + 1;
  }
  constructor() {
    this.parse = this.parse.bind(this);
  }
}
let sum = 0;
for (let i = 0; i < 1000; i++) sum += new Schema().parse(i);
console.log("many binds", sum);
