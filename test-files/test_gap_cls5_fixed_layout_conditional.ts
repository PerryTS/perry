// @ts-nocheck
// A layout the facts prove stable but not fixed: one birth key is assigned
// only on some paths of the constructor, so instances of the class do not all
// have the same layout. The dispatch on `this` keeps its layout recheck and
// each instance answers as Node's.
class Base {
  constructor(withB: boolean) {
    this.a = 1;
    if (withB) this.b = 2;
    this.c = 3;
  }
  val(): number {
    return this.a + this.c * 10;
  }
  run(): number {
    return this.val() + (this.b === undefined ? 100 : this.b);
  }
}
class Derived extends Base {
  constructor(withB: boolean) {
    super(withB);
    this.d = 4;
  }
  val(): number {
    return this.c * 7 + this.d;
  }
}
const items: Base[] = [];
for (let i = 0; i < 8; i++) {
  items.push(i % 2 === 0 ? new Base(i % 4 === 0) : new Derived(i % 4 === 1));
}
let total = 0;
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < items.length; i++) {
    total += items[i].run();
  }
}
console.log(total);
console.log(items.map((s) => s.run()).join(","));
