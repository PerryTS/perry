// #13960: private fields, methods and accessors on instances of an Array
// subclass. ethers' ABI decoder (`Result extends Array`) relies on this.
class Tagged extends Array<number> {
  #tag = "t";
  #count = 0;
  static #made = 0;
  constructor(...items: number[]) {
    super();
    for (const item of items) this.push(item);
    Tagged.#made++;
  }
  get #double() {
    return this.length * 2;
  }
  #bump() {
    return ++this.#count;
  }
  tag() {
    return this.#tag;
  }
  setTag(value: string) {
    this.#tag = value;
  }
  bump() {
    return this.#bump();
  }
  double() {
    return this.#double;
  }
  static has(value: unknown) {
    return #tag in (value as object);
  }
  static made() {
    return Tagged.#made;
  }
}

const a = new Tagged(1, 2, 3);
console.log(a.tag(), a.length, a.bump(), a.bump(), a.double());
a.setTag("changed");
console.log(a.tag());
// Growth past the initial capacity must keep the private elements.
for (let i = 0; i < 200; i++) a.push(i);
console.log(a.tag(), a.length, a.bump(), a.double());
console.log(Tagged.has(a), Tagged.has([1, 2]), Tagged.has({}));
console.log(Array.isArray(a), a instanceof Tagged, a.slice(0, 3).join(","));
// Private elements stay invisible to key enumeration and JSON.
const small = new Tagged(7, 8);
console.log(Object.keys(small).join(","), JSON.stringify(small), Object.getOwnPropertyNames(small).join(","));

// A plain array is not an instance: the access throws a TypeError.
try {
  Tagged.prototype.tag.call([1, 2]);
} catch (e) {
  console.log((e as Error).constructor.name);
}

// Many instances with allocation in between: each keeps its own fields.
const many: Tagged[] = [];
for (let i = 0; i < 2000; i++) {
  const t = new Tagged(i);
  t.setTag("n" + i);
  many.push(t);
  const junk = new Array(64).fill({ i });
  if (junk.length !== 64) throw new Error("unreachable");
}
let ok = 0;
for (let i = 0; i < many.length; i++) {
  if (many[i].tag() === "n" + i && many[i][0] === i) ok++;
}
console.log(ok, Tagged.made());

// The issue's shape: a fixed-length super() call.
class R extends Array<number> {
  #tag = "r";
  constructor() {
    super(2);
  }
  tag() {
    return this.#tag;
  }
}
console.log(new R().tag(), new R().length);
