const tag = Symbol("tag");
const id = Symbol.for("entityId");
const obj = { foo: 1, bar: "two", [tag]: true, [id]: 99 };
console.dir(obj);
