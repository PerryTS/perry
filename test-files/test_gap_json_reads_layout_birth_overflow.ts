import * as util from "node:util";
import * as v8 from "node:v8";
// A birth on a compiler-fixed layout (an `Object.create(P)` family with one
// key, the prototype image itself) has exactly its declared slots. A key
// added after birth goes to the overflow storage, and every reader
// (JSON.stringify with and without a replacer or an indent, util.format's
// %j) must read it from there, never past the object's end.

const proto: any = { a: 1 };
const f = Object.create(proto);
f.q = 1;
f.x = 1;
console.log(JSON.stringify(f), f.x);

class E {
  x = 0;
  y = 0;
  z = 0;
  set(x: number, y: number, z: number): this {
    this.x = x;
    this.y = y;
    this.z = z;
    return this;
  }
}
const log: string[] = [];
const withSetter: any = {
  set y(v: number) {
    log.push("sety:" + v + ":" + (this as any).x + ":" + (this as any).z);
    (this as any).z = "fromsetter";
  },
};
const g = Object.create(withSetter);
g.q = 1;
E.prototype.set.call(g, 1, 2, 3);
console.log(log.join("|"), JSON.stringify(g), Object.keys(g).join());
console.log(JSON.stringify(g, null, 1));
console.log(JSON.stringify(g, ["q", "x", "z"]));

function add(o: any): any {
  o.b = 2;
  o.c = 3;
  return o;
}
console.log(JSON.stringify(add(proto)));
const many = [];
for (let i = 0; i < 4; i++) {
  const m = Object.create(proto);
  m.q = i;
  many.push(add(m));
}
console.log(JSON.stringify(many));
console.log("%j", f);
console.log(Object.values(g).join(), Object.entries(proto).join("|"));
console.log(util.format("%j %j", f, [f]));
console.log(JSON.stringify(v8.deserialize(v8.serialize(f))));
console.log(JSON.stringify(structuredClone(f)));
function args(): any {
  const a: any = arguments;
  a.p = 1;
  a.q = 2;
  a.r = 3;
  return [a.p, a.q, a.r, Object.keys(a).join()];
}
console.log(JSON.stringify(args()));
