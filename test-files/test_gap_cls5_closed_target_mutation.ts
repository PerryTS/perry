// A closed-target method must stay correct when the program changes what the
// call resolves to: prototype mutation after construction, an own method on
// one instance, an accessor installed later, a subclass created late.
class Base {
  v = 1;
  twice(): number {
    return this.v * 2;
  }
  show(): string {
    return "base" + this.twice();
  }
}
class A extends Base {}
class B extends Base {
  twice(): number {
    return this.v * 20;
  }
}

const items: Base[] = [new A(), new B(), new A()];
function run(label: string): void {
  let s = label + ":";
  for (let i = 0; i < items.length; i++) s += items[i].show() + ",";
  console.log(s);
}
run("initial");

// 1. prototype method replaced after the instances exist
(Base.prototype as any).twice = function (this: Base): number {
  return this.v * 200;
};
run("proto-twice");

// 2. the inherited method itself replaced
(Base.prototype as any).show = function (this: Base): string {
  return "patched" + this.twice();
};
run("proto-show");

// 3. an own method on one instance shadows the prototype
(items[0] as any).show = function (): string {
  return "own";
};
run("own-show");

// 4. delete the own method: the prototype is visible again
delete (items[0] as any).show;
run("own-deleted");

// 5. an accessor installed later on the prototype of the base
Object.defineProperty(Base.prototype, "show", {
  get() {
    return () => "getter" + this_v(this);
  },
  configurable: true,
});
function this_v(o: any): number {
  return o.v;
}
run("getter");

// 6. a subclass created after the loops ran
class C extends B {
  show(): string {
    return "C" + this.twice();
  }
}
items.push(new C());
run("late-subclass");

// 7. setPrototypeOf on an instance
Object.setPrototypeOf(items[2], B.prototype);
run("set-proto");

// 8. an override that calls super, and a getter, on a fresh hierarchy
class Shape {
  name(): string {
    return "shape";
  }
  describe(): string {
    return this.name() + "!";
  }
}
class Dot extends Shape {
  name(): string {
    return "dot";
  }
}
class Tagged extends Dot {
  r = 7;
  describe(): string {
    return "tagged/" + super.describe();
  }
  get tag(): string {
    return "t" + this.r;
  }
}
const mixed: Shape[] = [new Dot(), new Tagged(), new Shape()];
let out = "";
for (let i = 0; i < 3; i++) {
  const o = mixed[i];
  out += o.describe() + ";";
}
console.log(out);
console.log((mixed[1] as Tagged).tag);
