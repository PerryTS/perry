// #11669: a class STATIC accessor's reflected function value (the closure an
// accessor property of the class function object holds since #11651) must
// call the compiled static entry with the static convention: the setter gets
// the assigned VALUE, and `this` is the receiver.
class Base {
  static log: string[] = [];
  static #hidden = 0;
  static _n = 0;
  static get n(): number {
    return this._n;
  }
  static set n(v: number) {
    this._n = v * 2;
    Base.log.push("set " + this.name + " " + String(v));
  }
  static get hidden(): number {
    return Base.#hidden;
  }
  static set hidden(v: number) {
    Base.#hidden = v + 100;
  }
  static set withReturn(v: number) {
    Base.log.push("withReturn " + String(v));
    return;
  }
}
class Sub extends Base {}

// Direct, dynamic-receiver and computed-key writes.
Base.n = 3;
console.log("direct", Base.n, Base._n);
let D: any = Sub;
D.n = 4;
console.log("dynamic-sub", Sub._n, Object.prototype.hasOwnProperty.call(Sub, "_n"), Base._n);
const k = "n";
(Base as any)[k] = 5;
console.log("computed", Base.n);

// Private static reached from a setter invoked through a subclass.
D.hidden = 7;
console.log("private", Base.hidden, D.hidden);

// A setter whose body returns: the assignment expression is still the value.
const r = ((Base as any).withReturn = 9);
console.log("assign-expr", r);

// The reflected halves called explicitly.
const desc = Object.getOwnPropertyDescriptor(Base, "n")!;
desc.set!.call(Base, 11);
console.log("reflected-set", Base._n, desc.get!.call(Base));
desc.set!.call(Sub, 12);
console.log("reflected-set-sub", Sub._n, desc.get!.call(Sub));
Reflect.set(Base, "n", 13);
console.log("reflect-set", Base._n);
Object.assign(Base, { n: 14 });
console.log("object-assign", Base._n);

// After a generic defineProperty (attributes only) the accessor still works.
Object.defineProperty(Base, "n", { enumerable: true });
Base.n = 15;
console.log("after-define", Base.n, Object.keys(Base).includes("n"));
console.log(Base.log.join("|"));
