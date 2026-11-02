// An inherited static symbol-keyed accessor runs with the receiver as `this`:
// OrdinaryGet/OrdinarySet pass the object the read started from, the holder
// only locates the accessor.
const S = Symbol("s");
class Own {
  static get [Symbol.species]() {
    return this;
  }
  static get [S]() {
    return this;
  }
  static set [S](v: unknown) {
    (this as any).stored = v;
  }
}
class Deep extends Own {}
class Deeper extends Deep {}
// 1. The species shape: the subclass answers itself.
console.log(Own[Symbol.species] === Own);
console.log(Deep[Symbol.species] === Deep);
console.log(Deeper[Symbol.species] === Deeper);
// 2. A user symbol getter, and an explicit receiver through Reflect.get.
console.log(Deep[S] === Deep, Deeper[S] === Deeper);
console.log(Reflect.get(Own, Symbol.species, Deep) === Deep);
// 3. The holder is untouched by reads through the subclass.
console.log(Object.prototype.hasOwnProperty.call(Own, "stored"), (Own as any).stored);
// 4. super[sym] in a static method: HomeObject locates, `this` stays.
class Sup extends Own {
  static m() {
    return super[Symbol.species];
  }
}
class Supper extends Sup {}
console.log(Sup.m() === Sup, Supper.m() === Supper);
// 5. The holder extends a built-in constructor.
class ROwn extends RegExp {
  static get [Symbol.species]() {
    return this;
  }
}
class RDeep extends ROwn {}
console.log(RDeep[Symbol.species] === RDeep, ROwn[Symbol.species] === ROwn);
