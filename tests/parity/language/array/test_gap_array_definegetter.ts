const fresh: any = [1, 2];
fresh.__defineGetter__("x", () => 43);
fresh.__defineGetter__("0", () => 44);
Object.defineProperty(fresh, "y", {get: () => 45});
console.log("fresh", fresh.x, fresh[0], fresh.y, fresh.__lookupGetter__("x")());

function read(o: any, k: any): any { return o[k]; }
const a: any = [1, 2];
const alias: any = a;
for (let i = 2; i < 40; i++) a.push(i);
const getter = () => 43;
a.__defineGetter__("x", getter);
a.__defineGetter__("0", () => 44);
console.log("annex-get", a.x, read(a, "x"), a[0], read(a, "0"), a[1]);
console.log("lookup-get", typeof a.__lookupGetter__("x"), a.__lookupGetter__("x")());
Object.defineProperty(a, "y", {get: () => 45, configurable: true});
Object.defineProperty(a, "1", {get: () => 46, configurable: true});
console.log("define-get", a.y, read(a, "y"), a[1], read(a, 1));
let named = 0;
let indexed = 0;
a.__defineSetter__("s", (v: any) => { named = v; });
a.__defineSetter__("2", (v: any) => { indexed = v; });
a.s = 51;
a[2] = 52;
console.log("set", named, indexed, read(a, "s"), read(a, 2));
console.log("lookup-set", typeof a.__lookupSetter__("s"), typeof a.__lookupSetter__("2"));

const b: any = [];
Object.defineProperty(b, "x", {get: () => 42, configurable: true});
Object.defineProperty(b, "20", {get: () => 7, configurable: true});
b.__defineGetter__("y", () => 43);
b.__defineSetter__("z", function(v: any) { this.v = v; });
b.z = 44;
console.log("growth", b.x, b[20], b.y, b.v, b.__lookupGetter__("y")());

const desc: any = Object.getOwnPropertyDescriptor(a, "x");
console.log("attrs", desc.enumerable, desc.configurable, a.propertyIsEnumerable("x"));
console.log("delete", Reflect.deleteProperty(a, "x"), a.x, a.__lookupGetter__("x"));
const frozen: any = [];
for (let i = 0; i < 40; i++) frozen.push(i);
Object.freeze(frozen);
const frozenDesc: any = Object.getOwnPropertyDescriptor(frozen, "0");
console.log("freeze", frozenDesc.writable, frozenDesc.configurable, Object.getOwnPropertyDescriptor(frozen, "length").writable);
const lengthArray: any = [1, 2];
for (let i = 0; i < 40; i++) lengthArray.push(i);
Object.defineProperty(lengthArray, "length", {writable: false});
console.log("length-attrs", Object.getOwnPropertyDescriptor(lengthArray, "length").writable);
