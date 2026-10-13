// Method certificates: prototypes swapped after the site certified a body.
// A swapped intermediate prototype moves only PERRY_PROTO_VALIDITY
// (sabotage `PERRY_METHOD_CERT_SABOTAGE=epoch` must abort); a swapped receiver prototype changes
// the receiver's own word (sabotage `PERRY_METHOD_CERT_SABOTAGE=word` must abort).
class A { tag() { return "A"; } }
class B extends A { tag() { return "B"; } }
class C extends A {}
class E extends C {}
const other = { tag() { return "other"; } };
function at(o: any) { return o.tag(); }
const a = new A(), b = new B(), c = new C(), e = new E();
const xs: any[] = [a, b, c, e];
for (let r = 0; r < 3; r++) console.log("warm", r, xs.map(at).join(","));
// The hop C.prototype now inherits from `other`, not A.prototype.
Object.setPrototypeOf(C.prototype, other);
console.log("hop swapped", xs.map(at).join(","));
// A receiver's own prototype.
Object.setPrototypeOf(a, B.prototype);
console.log("receiver swapped", xs.map(at).join(","));
Object.setPrototypeOf(C.prototype, A.prototype);
console.log("hop restored", xs.map(at).join(","));
// An accessor defined on the holder replaces the data method.
Object.defineProperty(A.prototype, "tag", { get() { return () => "getter"; }, configurable: true });
console.log("accessor", xs.map(at).join(","));
