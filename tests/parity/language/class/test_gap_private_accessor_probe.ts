// A private accessor (`get #x()` / `set #x(v)`) is a member of the declaring
// ClassBody, not a key of any layout. The by-name read site's layout probe
// answered "absent" for `#x` and returned undefined before the complete site
// operation could run the getter.

class Counter {
  #n = 5;
  get #twice() {
    return this.#n * 2;
  }
  set #twice(v: number) {
    this.#n = v;
  }
  get value() {
    return this.#twice;
  }
  read() {
    return this.#twice;
  }
  write(v: number) {
    this.#twice = v;
    return this.#twice;
  }
  static #s() {
    return 1;
  }
  static get #sg() {
    return 2;
  }
  static both() {
    return Counter.#s() + Counter.#sg;
  }
}

const c = new Counter();
console.log(c.value, c.read(), c.write(7), c.value, Counter.both());
console.log(Object.keys(c).length, Object.getOwnPropertyNames(Counter.prototype).join(","));
let s = 0;
for (let i = 0; i < 1000; i++) s += c.read();
console.log(s);
