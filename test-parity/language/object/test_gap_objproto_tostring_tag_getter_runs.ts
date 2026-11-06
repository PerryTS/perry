// Object.prototype.toString reads the receiver's @@toStringTag: the getter runs and its
// store is seen by later reads (perry printed "b": the read stayed proven to A).
class A { x = 1; }
class B { y = "b"; z = 3; }
const holder: { v: any } = { v: new A() };
const tagged = { get [Symbol.toStringTag]() { holder.v = new B(); return "Tagged"; } };
function read(h: { v: any }) { return h.v.x; }
console.log(read(holder));
console.log(Object.prototype.toString.call(tagged));
console.log(read(holder));
