// E6: setPrototypeOf swaps the prototype of an instance.
// WPFACTS: fact F3 prefix_stable Cat = 0
class Cat {
  n: number = 1;
  speak() { return "meow" + this.n; }
}
class Dog {
  speak() { return "woof"; }
}
const c = new Cat();
Object.setPrototypeOf(c, Dog.prototype);
console.log((c as any).speak(), c.n);
