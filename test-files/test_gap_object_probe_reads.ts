// Object model step 5: reads and writes through receivers no compile-time
// fact names (the colour-slot read, the layout probe, the prototype walk).
function get(o: any, k: string): any { return o[k]; }
function readX(o: any): any { return o.x; }
function readKind(o: any): any { return o.kind; }
function writeKind(o: any, v: any): void { o.kind = v; }

class Base { kind = 1; m() { return "base"; } get g() { return "getter"; } }
class Derived extends Base { x = 2; m() { return "derived"; } }
const objs: any[] = [new Base(), new Derived(), { kind: 3, x: 4 }, { x: 5 }, Object.create(null)];
for (const o of objs) console.log(readKind(o), readX(o), o.m ? o.m() : "none", o.g);

// inherited data, shadowing, deletion
const proto: any = { x: "proto-x", kind: "proto-kind" };
const child: any = Object.create(proto);
console.log(readX(child), readKind(child));
child.x = "own-x";
console.log(readX(child));
delete child.x;
console.log(readX(child));
proto.x = "proto-x2";
console.log(readX(child));

// Object.prototype additions and absent keys
const plain: any = { a: 1 };
console.log(plain.notThere, readX(plain));
(Object.prototype as any).probeAdded = "added";
console.log(plain.probeAdded, get(plain, "probeAdded"));
delete (Object.prototype as any).probeAdded;
console.log(plain.probeAdded);
console.log(typeof plain.hasOwnProperty, typeof plain.toString, plain.constructor === Object);

// null-prototype objects
const bare: any = Object.create(null);
bare.kind = "bare";
console.log(readKind(bare), bare.toString, readX(bare));

// short and computed keys, symbols
const short: any = {};
const k1 = "ab", k2 = "a" + "b";
short[k1] = 1;
console.log(get(short, k2), short.ab, get(short, "zz"));
const sym = Symbol("s");
const withSym: any = { [sym]: "sym-value", kind: "k" };
console.log(withSym[sym], get(withSym, "kind"));

// getters and setters on the receiver and on the prototype
const acc: any = { get kind() { return "acc-kind"; }, set kind(v) { console.log("set", v); } };
console.log(readKind(acc));
writeKind(acc, 7);
const accChild: any = Object.create(acc);
console.log(readKind(accChild));
writeKind(accChild, 8);

// frozen and non-writable stores
const frozen: any = Object.freeze({ kind: "frozen" });
try { writeKind(frozen, "changed"); } catch (e: any) { console.log("frozen store:", e.constructor.name); }
console.log(readKind(frozen));
const ro: any = {};
Object.defineProperty(ro, "kind", { value: "ro", writable: false, enumerable: true });
try { writeKind(ro, "changed"); } catch (e: any) { console.log("ro store:", e.constructor.name); }
console.log(readKind(ro));

// stores through a colour slot, then reads
const many: any[] = [];
for (let i = 0; i < 6; i++) many.push({ kind: i, x: i * 2 });
for (const o of many) writeKind(o, readKind(o) + 10);
console.log(many.map(readKind).join(","), many.map(readX).join(","));

// a prototype object's own store and an instance's read
class P { kind = "p"; }
const pinst: any = new P();
(P.prototype as any).extra = "proto-extra";
console.log(pinst.extra, readKind(pinst));
writeKind(P.prototype, "proto-kind-store");
console.log((P.prototype as any).kind, readKind(pinst));

// global reads
console.log(typeof Map, typeof Set, typeof Promise, typeof Array, typeof Date);
