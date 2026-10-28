// The `Iterator` global (ES2025): an abstract constructor whose `prototype` is
// %IteratorPrototype%, the object every built-in iterator inherits from.
const I: any = Iterator;
console.log(typeof I, I.name, I.length);
console.log((globalThis as any).Iterator === Iterator);
console.log(typeof Iterator.prototype);

const arrIt = [1][Symbol.iterator]();
console.log(Object.getPrototypeOf(Object.getPrototypeOf(arrIt)) === Iterator.prototype);
console.log(arrIt instanceof Iterator);
console.log(new Map().keys() instanceof Iterator, new Set().values() instanceof Iterator);

try { I(); } catch (e) { console.log("call:", (e as Error).constructor.name); }
try { new I(); } catch (e) { console.log("new:", (e as Error).constructor.name); }

class Countdown extends Iterator<number> {
  n = 2;
  next() {
    return this.n > 0 ? { done: false, value: this.n-- } : { done: true, value: undefined };
  }
}
const c = new Countdown();
console.log(c instanceof Iterator, Object.getPrototypeOf(Countdown.prototype) === Iterator.prototype);
console.log([...c]);

const P: any = Iterator.prototype;
console.log(P[Symbol.toStringTag], Object.prototype.toString.call(new Countdown()));
console.log(P.constructor === Iterator);
const tag = Object.getOwnPropertyDescriptor(P, Symbol.toStringTag)!;
console.log(typeof tag.get, typeof tag.set, tag.enumerable, tag.configurable);
const ctor = Object.getOwnPropertyDescriptor(P, "constructor")!;
console.log(typeof ctor.get, typeof ctor.set, ctor.enumerable, ctor.configurable);
const proto = Object.getOwnPropertyDescriptor(Iterator, "prototype")!;
console.log(proto.writable, proto.enumerable, proto.configurable);
console.log(P[Symbol.iterator].call(arrIt) === arrIt);
