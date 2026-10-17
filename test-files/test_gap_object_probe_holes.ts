// Object model step 5 (phase 2): the probe serves layouts with hole slots.
// A key the layout lists whose slot holds the hole is absent: a read, a
// method call, a store, `in` and hasOwn continue exactly as for a key the
// layout does not list (the prototype chain, then the key add).
class Base {
  greet(): string { return "base"; }
}
class Item extends Base {
  greet: any;
  name: any;
  constructor(i: number) {
    super();
    this.name = "n" + i;
    this.greet = function () { return "own" + i; };
  }
}
const items: any[] = [];
for (let i = 0; i < 6; i++) items.push(new Item(i));
// Delete an own method and an own field on some instances: their layouts
// keep the keys with hole slots (or move to another layout).
for (let i = 0; i < 6; i += 2) { delete items[i].greet; delete items[i].name; }
const out: string[] = [];
for (let round = 0; round < 3; round++) {
  for (const it of items) {
    out.push(it.greet());                       // method call: own, else inherited
    out.push(String(it.name));                  // absent read: undefined
    out.push(String("name" in it) + ":" + String(Object.prototype.hasOwnProperty.call(it, "name")));
    out.push(String(Object.keys(it).join(",")));
  }
}
// A store into the hole re-adds the key; the enumeration order follows.
for (let i = 0; i < 6; i += 2) { items[i].name = "re" + i; }
for (const it of items) out.push(Object.keys(it).join(",") + "=" + it.name + "/" + it.greet());
// A plain literal: delete, inherited lookup through Object.prototype, re-add.
const o: any = { a: 1, toString: () => "own", b: 2 };
delete o.toString;
out.push(String(o).slice(0, 8));
delete o.a;
out.push(String(o.a) + String("a" in o) + Object.keys(o).join(","));
o.a = 3;
out.push(Object.keys(o).join(",") + o.a);
// A prototype whose key slot is a hole: the walk continues past it.
const proto: any = { p: 1, q: 2 };
const child = Object.create(proto);
delete proto.p;
out.push(String(child.p) + String("p" in child) + String(child.q));
console.log(out.join("\n"));
