class Grand {
  data = 5;
  get value() { return this.data; }
}
class Parent extends Grand {
  get adjusted() { return this.adjust() + super.value; }
  set adjusted(n: number) { this.data = n; }
  adjust() { return this.data * 2; }
}
class Child extends Parent {
  adjust() { return this.data * 3; }
}
function probe(c: Child) { c.adjusted = 7; return c.adjusted; }
console.log(probe(new Child()));
class Other extends Parent {
  get adjusted() { return this.data + 100; }
  set adjusted(n: number) { this.data = n + 10; }
}
console.log(probe(new Other() as any));
