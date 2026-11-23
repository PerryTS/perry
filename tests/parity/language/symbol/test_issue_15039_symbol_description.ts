// #15039: `description` reads the description of a fresh and a registered
// symbol, and of a symbol without one.
const fresh = Symbol("foo");
const registered = Symbol.for("test262");
console.log(fresh.description, registered.description);
console.log(Symbol().description, Symbol("").description === "");

const anyFresh: any = Symbol("bar");
console.log(anyFresh.description);

const read = (value: any, key: string) => value[key];
console.log(read(Symbol("x"), "missing"), typeof read(Symbol("x"), "toString"));

// `Symbol.prototype.description` is an accessor whose getter brand-checks
// `this`: a symbol or a Symbol wrapper, anything else throws a TypeError.
const desc: any = Object.getOwnPropertyDescriptor(Symbol.prototype, "description");
console.log(typeof desc.get, desc.set, desc.enumerable, desc.configurable);
console.log(desc.get.name, desc.get.length);
const getter = desc.get;
console.log(getter.call(fresh), getter.call(Object(fresh)), getter.call(Symbol()));
console.log((Object(Symbol("boxed")) as any).description);
for (const value of [null, undefined, 0, "", {}, true]) {
  try {
    getter.call(value);
    console.log("no throw");
  } catch (e) {
    console.log(e instanceof TypeError);
  }
}
try {
  (Symbol.prototype as any).description;
} catch (e) {
  console.log("prototype read", e instanceof TypeError);
}
