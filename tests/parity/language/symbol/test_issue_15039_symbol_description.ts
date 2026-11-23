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
