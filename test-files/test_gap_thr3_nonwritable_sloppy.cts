// THR3: a constructor's Object.defineProperty(this, 'id', {value}) is a family
// key carrying its attributes; a sloppy store to it (class bodies are strict,
// so the store is in a sloppy function) fails silently.
class A {
  constructor(n) {
    Object.defineProperty(this, "id", { value: n });
    this.k = 2;
  }
  read() {
    return this.id * 10 + this.k;
  }
}
function bump(o) {
  o.id = 5;
  o.k = 7;
  return o.read();
}
const xs = [];
for (let i = 0; i < 4; i++) xs.push(new A(i + 1));
let s = 0;
for (const a of xs) s += bump(a);
console.log(s, xs[2].id, xs[2].k, Object.keys(xs[2]).join(","));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(xs[0], "id")));
