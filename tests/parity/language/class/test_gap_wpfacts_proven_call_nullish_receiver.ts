// NULLCALL1: a method call the facts prove runs one body, on a receiver
// that may also be undefined / null: one nullish test, the TypeError of the
// complete call before the arguments run, else the direct call.
class A {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  m(x: number): number {
    return this.v + x;
  }
}
class B {
  w: number;
  constructor(w: number) {
    this.w = w;
  }
  twice(x: number): number {
    return this.w * x;
  }
}
class H {
  b: B | null;
  constructor() {
    this.b = null;
  }
}
let counter = 0;
function bump(): number {
  counter++;
  return counter;
}
const arg = process.argv[2] ?? "0";
const arr: A[] = [new A(1), new A(2), new A(3)];
// RED 1: an element past the length floor is undefined; the method is A.m.
function callArr(i: number): void {
  const a = arr[i];
  try {
    console.log("arr", a.m(bump()));
  } catch (e) {
    console.log("arr threw", (e as Error).message);
  }
  console.log("arr counter", counter);
}
// RED 2: a field born null, assigned an instance under a condition.
const h = new H();
if (arg === "1") h.b = new B(5);
function callField(): void {
  try {
    console.log("field", h.b.twice(bump()));
  } catch (e) {
    console.log("field threw", (e as Error).message);
  }
  console.log("field counter", counter);
}
// RED 3: optional chaining on the same receiver kind.
function callOpt(i: number): void {
  const a = arr[i];
  console.log("opt", a?.m(bump()), counter);
}
// KEEP 1: the receiver may be a number: generic.
function keepNum(): void {
  const n: any = arg === "2" ? 7 : arr[0];
  try {
    console.log("num", n.m(bump()));
  } catch (e) {
    console.log("num threw", (e as Error).message);
  }
}
// KEEP 2: a receiver proven an object: no test.
const solo = new A(Number(arg) + 10);
function keepPlain(): void {
  console.log("plain", solo.m(bump()));
}
// KEEP 3: two bodies behind the site (callee not one body): generic.
class C {
  m(x: number): number {
    return -x;
  }
}
const mixed: any[] = [new A(9), new C()];
function keepTwo(i: number): void {
  const q = mixed[i];
  try {
    console.log("two", q.m(bump()));
  } catch (e) {
    console.log("two threw", (e as Error).message);
  }
}
callArr(Number(arg));
callArr(Number(arg) + 7);
keepTwo(Number(arg));
callField();
h.b = new B(3);
callField();
callOpt(Number(arg) + 2);
keepNum();
keepPlain();
