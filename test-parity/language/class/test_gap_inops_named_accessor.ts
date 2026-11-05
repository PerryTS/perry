class Counter {
  value = 0;
  get scaled() { return this.value * 3 + 7; }
  set input(x: number) { this.value = x & 255; }
}
function contained(n: number) {
  const c = new Counter();
  let total = 0;
  for (let i = 0; i < n; i++) { c.input = i; total += c.scaled; }
  return total;
}
console.log("contained", contained(1000));
class Leaking {
  value = 0;
  get self() { return this; }
  set input(x: number) { this.value = x; }
}
const leaking = new Leaking();
leaking.self.input = 19;
console.log("leaking", leaking.value, leaking.self === leaking);
class Shadowed { value = 1; get score() { return this.value; } }
const shadowed = new Shadowed();
console.log("before", shadowed.score);
Object.defineProperty(shadowed, "score", { value: 42 });
console.log("own", shadowed.score);
Object.defineProperty(Shadowed.prototype, "score", { get() { return 99; } });
console.log("prototype", new Shadowed().score);
class Base { get score() { return 1; } }
class Child extends Base { get score() { return 2; } }
console.log("subclass", new Child().score);
const key = "answer";
class Computed { value = 1; get [key]() { return this.value; } set [key](v: number) { this.value = v; } }
const computed = new Computed();
computed.answer = 29;
console.log("computed", computed.answer);
