// Presence must not read values or invoke accessors.
let gets = 0;
const proto: any = Object.create(null);
Object.defineProperty(proto, "value", { get() { gets++; return undefined; }, configurable: true });
const mid: any = Object.create(proto);
const obj: any = Object.create(mid);
function check(o: any, k: any) {
  console.log(k in o, Reflect.has(o, k), Object.hasOwn(o, k), Object.prototype.hasOwnProperty.call(o, k));
}
for (let i = 0; i < 12; i++) check(obj, "value");
check(obj, "missing");
Object.defineProperty(obj, "value", { value: undefined, configurable: true });
check(obj, "value");
delete obj.value;
check(obj, "value");
delete proto.value;
check(obj, "value");
mid.value = undefined;
check(obj, "value");
Object.setPrototypeOf(obj, null);
check(obj, "value");
console.log("gets", gets);
const dictionary: any = {};
for (let i = 0; i < 1100; i++) dictionary["d" + i] = undefined;
const child: any = Object.create(dictionary);
for (let i = 0; i < 12; i++) check(child, "d1099");
check(dictionary, "d1099");
delete dictionary.d1099;
check(child, "d1099");
check(dictionary, "d1099");
dictionary.d1099 = undefined;
check(child, "d1099");
check(dictionary, "d1099");
const sym = Symbol("presence");
Object.defineProperty(proto, sym, { get() { gets++; }, configurable: true });
console.log(sym in proto, Object.hasOwn(proto, sym), gets);
for (const value of [null, undefined, 3, "x", true]) {
  try { console.log("key" in (value as any)); } catch (e) { console.log(e instanceof TypeError); }
}
const numeric: any = { 7: undefined };
check(numeric, 7);
check(numeric, 8);
let coerces = 0;
const mutatingKey = { toString() { coerces++; numeric.added = undefined; return "added"; } };
check(numeric, mutatingKey);
console.log("coerces", coerces);
