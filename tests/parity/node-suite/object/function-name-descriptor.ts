// #2059: a function's built-in `name` own-property read back `undefined`
// (and `getOwnPropertyDescriptor(fn, "name")` crashed). `name` is a
// `{ writable:false, enumerable:false, configurable:true }` own data property.
// Descriptor fields are logged individually so the byte-for-byte comparison
// doesn't depend on console's multi-line object formatting.

function foo(a: number, b: number) {}
console.log("name:", foo.name);
console.log("length:", foo.length);

const d = Object.getOwnPropertyDescriptor(foo, "name");
console.log(
  "desc:",
  d?.value,
  d?.writable,
  d?.enumerable,
  d?.configurable,
);

const arrow = (x: number) => x;
console.log("arrow:", arrow.name);

class C {
  m() {}
}
console.log("class:", C.name);
console.log("method:", new C().m.name);
