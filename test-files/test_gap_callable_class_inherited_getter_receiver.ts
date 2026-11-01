// A callable class instance (`Callable`'s constructor returns
// `Object.setPrototypeOf(fn.bind(this), new.target.prototype)`, ArkType's
// pattern) reads its class getters through the function object's static
// prototype chain. A getter one or more levels above the first prototype ran
// with that prototype as `this`: ArkType's `BaseRoot.rawIn` (a `super` getter
// read on a `DomainNode`) returned `BaseRoot.prototype` and `new UnionNode`
// failed reading `$.parseDefinition`.
class Callable {
  constructor(fn: (...a: any[]) => any) {
    return Object.setPrototypeOf(fn.bind(this), (this as any).constructor.prototype);
  }
}
class Node extends Callable {
  tag = "node";
  constructor() { super((x: number) => x * 2); }
  get me(): any { return this; }
  get label(): string { return "label:" + this.tag; }
  cacheGetter(name: string, value: any) { Object.defineProperty(this, name, { value }); return value; }
  get rawIn(): any { return this.cacheGetter("rawIn", this); }
}
class Root extends Node {
  get rawIn(): any { return super.rawIn; }
}
class Basis extends Root {}
class Domain extends Basis {}

const one: any = new Root();
const two: any = new Domain();
console.log("depth 1:", one.me === one, one.label);
console.log("depth 2:", two.me === two, two.label);
const raw = two.rawIn;
console.log("super getter through a subclass:", raw === two, typeof raw, raw(21));
console.log("cached on the instance:", Object.getOwnPropertyNames(two).includes("rawIn"));
console.log("not cached on a prototype:", !Object.getOwnPropertyNames(Root.prototype).includes("rawIn") || typeof Object.getOwnPropertyDescriptor(Root.prototype, "rawIn")!.get === "function");

const fn: any = Object.setPrototypeOf(function () { return 1; }, Domain.prototype);
console.log("plain function:", fn.me === fn, fn.label);
const obj: any = Object.create(Domain.prototype);
console.log("plain object:", obj.me === obj);
