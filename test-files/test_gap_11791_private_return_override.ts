// #11791: a base constructor returning another object makes the derived
// class add its private elements to that object; nothing leaks into
// reflection or JSON, and a second stamp throws.
class Base { constructor(o: any) { return o; } }
class Stamp extends Base {
  #tag = "t";
  #s() { return "s"; }
  static has(o: any) { return #s in o; }
  static hasTag(o: any) { return #tag in o; }
  static tag(o: any) { return o.#tag + o.#s(); }
}
const plain: any = { x: 1 };
new Stamp(plain);
console.log("stamped:", Stamp.has(plain), Stamp.hasTag(plain), Stamp.tag(plain));
console.log("reflect:", Object.keys(plain), Object.getOwnPropertyNames(plain), JSON.stringify(plain));
plain.y = 2;
delete plain.x;
console.log("after edits:", Stamp.has(plain), Stamp.tag(plain), JSON.stringify(plain));
try { new Stamp(plain); } catch (e: any) { console.log(e.constructor.name, e.message); }

class Other { #o = 1; static has(o: any) { return #o in o; } }
const other = new Other();
new Stamp(other);
console.log("on instance:", Stamp.has(other), Other.has(other), JSON.stringify(other));

const many: any[] = [];
for (let i = 0; i < 50; i++) { const o: any = { i }; new Stamp(o); many.push(o); }
console.log("many:", many.every((o) => Stamp.has(o) && Stamp.tag(o) === "ts"), JSON.stringify(many[49]));
