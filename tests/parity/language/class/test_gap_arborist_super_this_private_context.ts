// Arborist's PackumentCache reads a static private field before super().
// A private guard's lexical brand context must not evaluate constructor this.
class Base {}
class Cache extends Base {
  static #limit = 8;
  #value = 2;
  #read() { return this.#value; }

  constructor(previous: any = null) {
    console.log("static", Cache.#limit, #limit in Cache);
    Cache.#limit++;
    console.log("brand", #value in (previous || {}));
    if (previous) {
      console.log("peer", previous.#value, previous.#read());
      previous.#value++;
    }
    super();
    console.log("ready", this.#value);
  }
}
const first = new Cache();
new Cache(first);

// The same access through a fresh class evaluation must retain its own brand.
function make(seed: number) {
  return class Local extends Base {
    static #limit = seed;
    static #read() { return Local.#limit; }
    constructor() {
      console.log("local", Local.#read(), #limit in Local);
      Local.#limit += 1;
      super();
    }
  };
}
const A = make(3);
const B = make(7);
new A();
new B();
new A();

// An actual this access still throws before super(), including private reads.
class Invalid extends Base {
  #value = 1;
  constructor() {
    console.log(this.#value);
    super();
  }
}
try { new Invalid(); } catch (e) { console.log("this", e.name); }

// A wrong private receiver produces TypeError, without consulting this.
class Wrong extends Base {
  static #limit = 1;
  constructor() {
    const foreign: any = {};
    console.log(foreign.#limit);
    super();
  }
}
try { new Wrong(); } catch (e) { console.log("foreign", e.name); }
