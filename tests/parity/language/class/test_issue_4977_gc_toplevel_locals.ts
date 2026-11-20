// #4977: explicit gc() in default auto stack-scan mode reclaimed live
// top-level locals — string fields read back as dangling-pointer garbage.
class Widget {
  x = 1;
  constructor(public name: string) {}
}

const keep = { nested: { deep: "leaf-string-4916" } };
const w = new Widget("widget-name-4977");

gc();

console.log(keep.nested.deep.length);
console.log(keep.nested.deep);
console.log(w.name);
console.log(w.x);
