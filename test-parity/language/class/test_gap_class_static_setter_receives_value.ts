// A class's static setter receives the assigned VALUE on every route: a
// direct store, a store through a variable or a shared (warmed) put site, an
// inherited store, Reflect.set, and the reflected setter function called with
// an explicit receiver. A static accessor's compiled entry takes no receiver
// parameter (unlike an instance accessor's), so a route that calls it with
// the instance convention hands the setter the class instead of the value.
//
// Output must be byte-identical to node.
class HasStatic {
  static seen: unknown = null;
  static get tag(): unknown {
    return HasStatic.seen;
  }
  static set tag(v: unknown) {
    HasStatic.seen = "set:" + String(v);
  }
}
class SubStatic extends HasStatic {}
let P: any = HasStatic;
let Q: any = SubStatic;
function put(o: any, v: unknown): void {
  o.tag = v;
}
HasStatic.tag = 7;
console.log("direct", HasStatic.seen);
P.tag = 8;
console.log("var", HasStatic.seen);
put(P, 1);
console.log("fn-cold", HasStatic.seen);
Q.tag = 3;
console.log("inherited", HasStatic.seen);
put({ a: 1 }, 0);
put({ b: 1, c: 2 }, 0);
put(new Map(), 0);
put(P, 5);
console.log("fn-warm", HasStatic.seen);
put(Q, 4);
console.log("fn-warm-inherited", HasStatic.seen);
Reflect.set(P, "tag", 6);
console.log("reflect", HasStatic.seen);
const d = Object.getOwnPropertyDescriptor(HasStatic, "tag")!;
d.set!.call(HasStatic, 9);
console.log("descriptor-set", HasStatic.seen, d.get!.call(HasStatic));

// `this` in a static accessor is the receiver the access went through.
class ThisStatic {
  static store: string = "";
  static get who(): string {
    return (this as any).name;
  }
  static set who(v: string) {
    (this as any).store = (this as any).name + "=" + v;
  }
}
class ThisSub extends ThisStatic {}
ThisSub.who = "a";
console.log("this-inherited", ThisSub.who, (ThisSub as any).store, ThisStatic.store);
const dw = Object.getOwnPropertyDescriptor(ThisStatic, "who")!;
dw.set!.call(ThisSub, "b");
console.log("this-descriptor", dw.get!.call(ThisSub), (ThisSub as any).store);

// A symbol-keyed static accessor, reflected.
const k = Symbol("k");
class SymStatic {
  static v: number = 0;
  static get [k](): number {
    return SymStatic.v;
  }
  static set [k](n: number) {
    SymStatic.v = n * 2;
  }
}
const ds = Object.getOwnPropertyDescriptor(SymStatic, k)!;
ds.set!.call(SymStatic, 11);
console.log("symbol-descriptor", SymStatic.v, ds.get!.call(SymStatic));
