class Base {
  m(value: { n: number }): number { return value.n; }
}
class Child extends Base {
  tag = 10;
  forward(value: { n: number }): number { return super.m(value); }
}
const child = new Child();
let reads = 0;
const target = function (this: { tag: number }, value: { n: number }): number {
  return this.tag + value.n;
};
const callable = new Proxy(target, {
  apply(fn, receiver, args) { return Reflect.apply(fn, receiver, args); },
});
Object.setPrototypeOf(Child.prototype, {
  get m() { reads++; return callable; },
});
console.log("proxy", child.forward({ n: 7 }), reads);
Object.setPrototypeOf(Child.prototype, { m: target });
console.log("function", child.forward({ n: 8 }));
Object.setPrototypeOf(Child.prototype, { m: 123 });
try { child.forward({ n: 9 }); } catch (e) {
  console.log("noncallable", e instanceof TypeError);
}

const directProxy = new Proxy(target, {});
Object.setPrototypeOf(Child.prototype, { m: directProxy });
console.log("default-proxy", child.forward({ n: 9 }));
let outerCalls = 0;
const nestedProxy = new Proxy(callable, {
  apply(fn, receiver, args) { outerCalls++; return Reflect.apply(fn, receiver, args); },
});
Object.setPrototypeOf(Child.prototype, { m: nestedProxy });
console.log("nested-proxy", child.forward({ n: 10 }), outerCalls);
let invalidApplies = 0;
const objectProxy = new Proxy({ x: 1 }, {
  apply() { invalidApplies++; return 999; },
});
Object.setPrototypeOf(Child.prototype, { m: objectProxy });
try { child.forward({ n: 11 }); } catch (e) {
  console.log("object-proxy", e instanceof TypeError, invalidApplies);
}
const revocable = Proxy.revocable(target, {});
Object.setPrototypeOf(Child.prototype, { m: revocable.proxy });
revocable.revoke();
try { child.forward({ n: 12 }); } catch (e) {
  console.log("revoked-proxy", e instanceof TypeError);
}
Object.setPrototypeOf(Child.prototype, {
  get m() { throw new Error("getter-sentinel"); },
});
try { child.forward({ n: 13 }); } catch (e) {
  console.log("getter-throw", (e as Error).message);
}
