// Members named by a registered symbol: the key is evaluated by the class
// definition, and the members are found under that symbol.
class Plain {
  [Symbol.for("m")](n: number) {
    return n + 1;
  }
  get [Symbol.for("g")]() {
    return "got";
  }
  *[Symbol.for("gen")]() {
    yield 1;
    yield 2;
  }
  static [Symbol.for("s")]() {
    return "static";
  }
  [Symbol.iterator]() {
    return [7, 8][Symbol.iterator]();
  }
}
const p: any = new Plain();
console.log(Object.getOwnPropertySymbols(Plain.prototype).map(String).sort().join(","));
console.log(p[Symbol.for("m")](2), p[Symbol.for("g")], [...p[Symbol.for("gen")]()].join());
console.log((Plain as any)[Symbol.for("s")](), [...p].join());

class Derived extends Plain {
  [Symbol.for("m")](n: number) {
    return super[Symbol.for("m")](n) * 10;
  }
}
console.log((new Derived() as any)[Symbol.for("m")](2));

function make(tag: string) {
  return class {
    [Symbol.for("m")]() {
      return tag;
    }
  };
}
const A: any = make("a");
const B: any = make("b");
console.log(new A()[Symbol.for("m")](), new B()[Symbol.for("m")]());
