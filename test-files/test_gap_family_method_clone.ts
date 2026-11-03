// Method calls on family-born class instances: the family layout word may
// enter the layout clone only while the object still holds exactly what the
// birth layout describes. Every receiver that leaves it (an own method, an
// added key, a deleted field, a non-number store, a frozen object, a late
// constructor key) must keep ordinary semantics.

class Base {
  scale: number;
  constructor(k: number) {
    this.scale = k;
  }
  step(x: number): number {
    return (x & 255) * this.scale + 7;
  }
  bump(): number {
    this.scale = this.scale + 1;
    return this.scale;
  }
}
class Derived extends Base {
  step(x: number): number {
    return (x & 255) * this.scale + 11;
  }
}

function run(cs: Base[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += cs[i % cs.length].step(i);
  return s;
}

const plain = [new Base(3), new Derived(5)];
console.log("plain", run(plain, 1000));

// An own method shadows the prototype's.
const own = new Base(2);
(own as any).step = (x: number) => x * 1000;
console.log("own", run([own, new Derived(1)], 10));

// An added key keeps the method but leaves the birth layout.
const added = new Derived(4);
(added as any).extra = "e";
console.log("added", run([new Base(1), added], 10), (added as any).extra);

// A deleted field reads undefined through the method.
const deleted = new Base(6);
delete (deleted as any).scale;
console.log("deleted", deleted.step(3), deleted.bump());

// A non-number field value.
const str = new Derived(1);
(str as any).scale = "7";
console.log("string", str.step(2), run([str, new Base(2)], 6));

// Frozen: the store in bump is ignored (sloppy) or throws (class bodies are strict).
const frozen = Object.freeze(new Base(9));
try {
  console.log("frozen", frozen.bump());
} catch (e) {
  console.log("frozen threw", (e as Error).constructor.name);
}
console.log("frozen step", frozen.step(1), frozen.scale);

// A getter on the prototype chain for the field name.
const proto = new Derived(3);
Object.defineProperty(proto, "scale", { get: () => 100, configurable: true });
console.log("accessor", proto.step(1));

// Stores through the clone stay visible.
const b = new Base(1);
for (let i = 0; i < 5; i++) b.bump();
console.log("bumped", b.scale, b.step(1));

// A class whose constructor calls a method before storing its field: the
// field is not present at that call.
class Late {
  v: number;
  seen: string;
  constructor(v: number) {
    this.seen = this.peek();
    this.v = v;
  }
  peek(): string {
    return typeof (this as any).v + ":" + ("v" in this);
  }
  get2(): number {
    return this.v * 2;
  }
}
const late = new Late(21);
console.log("late", late.seen, late.get2(), late.peek());

// Two fields stored in the other order than declared.
class Swapped {
  a: number;
  b: number;
  constructor(x: number) {
    this.b = x + 1;
    this.a = x;
  }
  sum(): number {
    return this.a * 10 + this.b;
  }
}
let sw = 0;
for (let i = 0; i < 100; i++) sw += new Swapped(i).sum();
console.log("swapped", sw, Object.keys(new Swapped(1)).join(","));

// Enumeration order of a family instance with every key present.
console.log("keys", Object.keys(new Derived(2)).join(","), JSON.stringify(new Derived(2)));
