// `Reflect.construct(T, args, N)`: the object made has `N.prototype` as its
// [[Prototype]], so a member the program put there is what a call on the
// result finds first. For a built-in `T` (and for a function `N`) the
// whole-program solve holds the result as unknown and dropped the new
// target, so that member's body was called inactive while it ran (check
// mode `PERRY_VERIFY_FACTS=1`: "inactive body: body entered"; with
// `PERRY_FACTS_DCE=1` the removed body trapped).
function NT() {}
NT.prototype = { has: () => "nt-has", extra: () => "nt-extra" };
const wm: any = Reflect.construct(WeakMap, [], NT);
console.log("weakmap:", wm.has({}), wm.extra());
console.log("weakmap-proto:", Object.getPrototypeOf(wm) === NT.prototype);

function Plain() {}
(Plain as any).prototype.tag = function (this: any) {
  return "plain-tag";
};
const m: any = Reflect.construct(Map, [], Plain);
console.log("map:", m.tag(), Object.getPrototypeOf(m) === Plain.prototype);

class Cls {
  describe(): string {
    return "cls-describe";
  }
}
const s: any = Reflect.construct(Set, [], Cls);
console.log("set-class:", s.describe(), s instanceof Cls);

function WithGetter() {}
WithGetter.prototype = {
  get answer() {
    return 42;
  },
};
const ws: any = Reflect.construct(WeakSet, [], WithGetter);
console.log("getter:", ws.answer);

// A program target with a function new target.
function Target(this: any, v: number) {
  this.v = v;
}
function Other() {}
Other.prototype = { twice(this: any) { return this.v * 2; } };
const o: any = Reflect.construct(Target, [21], Other);
console.log("function-target:", o.twice(), Object.getPrototypeOf(o) === Other.prototype);

// Controls: the default new target, and a class new target of a class.
class Two {
  r = 1;
  who(): string {
    return "two";
  }
}
class Three {
  r = 3;
  who(): string {
    return "three";
  }
}
const d: any = Reflect.construct(Two, []);
const e: any = Reflect.construct(Two, [], Three);
console.log("controls:", d.who(), d.r, e.who(), e.r);
