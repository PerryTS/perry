// Object.prototype.toLocaleString invokes the receiver's toString: its store is seen
// by later reads (perry printed "b": the read stayed proven to A).
class A { x = 1; }
class B { y = "b"; z = 3; }
const holder: { v: any } = { v: new A() };
const o = { toString() { holder.v = new B(); return "mine"; } };
function read(h: { v: any }) { return h.v.x; }
console.log(read(holder));
console.log(o.toLocaleString());
console.log(read(holder));
