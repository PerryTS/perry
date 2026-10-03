// A class prototype is a HOLDER FACT's forbidden identity: the bare class
// ShapeId of an instance or of a prototype object does not pin the registry's
// live link, so the plain read-holder walk must refuse every chain that passes
// through one (the class read site owns those, and validates the registry
// link on every hit). Receivers here are plain objects created from class
// prototypes, so the walk reaches a class prototype as an INTERMEDIATE hop;
// the sites read keys that live above it. Parameter receivers and
// runtime-length loops keep the sites alive. check.sh asserts the plain
// holder entry never primed for them (read_holder_primes=0) while the class
// read entry did, so the fixture cannot pass by never reaching the walk.
class B { b = 1; }
class C extends B { c = 2; }
(B.prototype as any).k = 5;
(B.prototype as any).deep = 50;

function read(o: any): number { const v = o.k; return v === undefined ? -1 : v; }
function readDeep(o: any): number { const v = o.deep; return v === undefined ? -1 : v; }
function readAbsent(o: any): number { const v = o.nope; return v === undefined ? -1 : v; }
function sum(objs: any[], f: (o: any) => number): number {
  let s = 0;
  for (let i = 0; i < 1500; i++) s += f(objs[i % objs.length]);
  return s;
}
const viaC1: any = Object.create(C.prototype);
const viaC2: any = Object.create(C.prototype);
const inst: any = new C();
console.log("hop", sum([viaC1, viaC2], read), sum([viaC1], readDeep), sum([viaC2], readAbsent));
console.log("instance", sum([inst], read), sum([inst], readAbsent));

(B.prototype as any).k = 6;
console.log("holder-store", sum([viaC1, viaC2], read), sum([inst], read));
viaC1.k = 100;
console.log("own-shadow", sum([viaC1, viaC2], read));
(C.prototype as any).k = 9;
console.log("class-proto-add", sum([viaC2], read), sum([inst], read));
delete (C.prototype as any).k;
console.log("class-proto-delete", sum([viaC2], read), sum([inst], read));
