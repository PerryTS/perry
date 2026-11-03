"use strict";
// A fixed layout is a fact about the whole program. Each family below has the
// shape of the proven template-method program (a base, two classes, a method
// that dispatches on `this`), and one thing the program does to an instance
// after construction. A class any of these reaches has no fixed layout: its
// dispatch keeps the layout recheck, and the answers are Node's.
const out: string[] = [];

class B1 {
  a: number = 1;
  b: number = 2;
  c: number = 3;
  val(): number { return this.a + this.b * 10 + this.c * 100; }
  run(): number { return this.val() + 1; }
  bump(): number { this.a = this.a + 1; return this.a; }
}
class D1 extends B1 {
  val(): number { return this.c * 7 + this.b; }
}
const f1: B1[] = [new B1(), new D1(), new B1(), new D1()];
// delete moves the packed slots
out.push(f1.map((s) => s.run()).join(","));
delete (f1[0] as any).b;
out.push(f1.map((s) => s.run()).join(","));
out.push(f1.map((s) => s.run()).join(","));

class B2 {
  a: number = 1;
  b: number = 2;
  c: number = 3;
  val(): number { return this.a + this.b * 10 + this.c * 100; }
  run(): number { return this.val() + 1; }
  bump(): number { this.a = this.a + 1; return this.a; }
}
class D2 extends B2 {
  val(): number { return this.c * 7 + this.b; }
}
const f2: B2[] = [new B2(), new D2(), new B2(), new D2()];
// defineProperty installs an accessor over a field
out.push(f2.map((s) => s.run()).join(","));
Object.defineProperty(f2[0], "c", { get() { return 40; }, configurable: true });
out.push(f2.map((s) => s.run()).join(","));
out.push(f2.map((s) => s.run()).join(","));

class B3 {
  a: number = 1;
  b: number = 2;
  c: number = 3;
  val(): number { return this.a + this.b * 10 + this.c * 100; }
  run(): number { return this.val() + 1; }
  bump(): number { this.a = this.a + 1; return this.a; }
}
class D3 extends B3 {
  val(): number { return this.c * 7 + this.b; }
}
const f3: B3[] = [new B3(), new D3(), new B3(), new D3()];
// freeze makes the stores of a method throw
out.push(f3.map((s) => s.run()).join(","));
Object.freeze(f3[2]);
let thrown = "";
try { f3[2].bump(); } catch (e) { thrown = (e as Error).name; }
out.push(f3.map((s) => s.run()).join(",") + " " + thrown + " " + f3[2].a);
out.push(f3.map((s) => s.run()).join(","));

class B4 {
  a: number = 1;
  b: number = 2;
  c: number = 3;
  val(): number { return this.a + this.b * 10 + this.c * 100; }
  run(): number { return this.val() + 1; }
  bump(): number { this.a = this.a + 1; return this.a; }
}
class D4 extends B4 {
  val(): number { return this.c * 7 + this.b; }
}
const f4: B4[] = [new B4(), new D4(), new B4(), new D4()];
// setPrototypeOf swaps the methods under the instance
out.push(f4.map((s) => s.run()).join(","));
Object.setPrototypeOf(f4[0], D4.prototype);
out.push(f4.map((s) => s.run()).join(","));
out.push(f4.map((s) => s.run()).join(","));

class B5 {
  a: number = 1;
  b: number = 2;
  c: number = 3;
  val(): number { return this.a + this.b * 10 + this.c * 100; }
  run(): number { return this.val() + 1; }
  bump(): number { this.a = this.a + 1; return this.a; }
}
class D5 extends B5 {
  val(): number { return this.c * 7 + this.b; }
}
const f5: B5[] = [new B5(), new D5(), new B5(), new D5()];
// a key is added after construction
out.push(f5.map((s) => s.run()).join(","));
(f5[2] as any).extra = 9;
(f5[2] as any).b = 5;
out.push(f5.map((s) => s.run()).join(",") + " " + Object.keys(f5[2]).join("."));
out.push(f5.map((s) => s.run()).join(","));

console.log(out.join("\n"));
