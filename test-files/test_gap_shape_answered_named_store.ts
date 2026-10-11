// Warm stores must observe subsequent changes to the prototype chain.
function put(o: any, value: number) { o.rangePos = value; }
const proto: any = {};
for (let i = 0; i < 24; i++) { const o = Object.create(proto); put(o, i); }
let setterTotal = 0;
Object.defineProperty(proto, "rangePos", { set(v) { setterTotal += v; }, configurable: true });
const afterSetter = Object.create(proto);
put(afterSetter, 7);
console.log("setter", setterTotal, Object.hasOwn(afterSetter, "rangePos"));
Object.defineProperty(proto, "rangePos", { value: 9, writable: false, configurable: true });
const afterReadonly = Object.create(proto);
console.log("readonly", Reflect.set(afterReadonly, "rangePos", 12), afterReadonly.rangePos, Object.hasOwn(afterReadonly, "rangePos"));
proto.padding = 1; delete proto.padding;
console.log("readonly-after-delete", Reflect.set(Object.create(proto), "rangePos", 12));
delete proto.rangePos;
const afterDelete = Object.create(proto); put(afterDelete, 13);
console.log("deleted", afterDelete.rangePos);
const own: any = { rangePos: 1 };
Object.preventExtensions(own); put(own, 2);
console.log("noextend", own.rangePos, Reflect.set(own, "other", 3));
Object.freeze(own);
console.log("frozen", Reflect.set(own, "rangePos", 4), own.rangePos);
class OwnRange { rangePos = 1; }
const classOwn = new OwnRange();
Object.preventExtensions(classOwn); put(classOwn, 2);
console.log("class-noextend", classOwn.rangePos, Reflect.set(classOwn, "other", 3));
Object.freeze(classOwn);
console.log("class-frozen", Reflect.set(classOwn, "rangePos", 4), classOwn.rangePos);
let proxyTotal = 0;
const proxyProto = new Proxy({}, { set(_target, key, value, receiver) {
  proxyTotal += value; Object.defineProperty(receiver, key, { value: value + 1, writable: true }); return true;
} });
const proxied = Object.create(proxyProto); put(proxied, 5);
console.log("proxy", proxyTotal, proxied.rangePos);
// Array named keys stay independent of Array's exotic length/index rules.
const a: any = [10, 20, 30]; a.pos = -1; a.end = 3; a.transformFlags = 0;
for (let i = 0; i < 24; i++) { a.pos = i; a.end = i + 3; }
console.log("array", a.pos, a.end, a.transformFlags, a.length);
a.length = 1; a[2] = 40;
console.log("indices", a.length, a[0], a[1], a[2], a.pos);
Object.preventExtensions(a);
console.log("array-noextend", Reflect.set(a, "pos", 31), Reflect.set(a, "other", 1), a.pos);
Object.freeze(a);
console.log("array-frozen", Reflect.set(a, "pos", 32), Reflect.set(a, "length", 0), Reflect.set(a, "0", 99));
const fullArray: any = [1, 2, 3, 4]; fullArray.pos = 7;
for (let i = 0; i < 24; i++) fullArray.push(i);
console.log("array-grow", fullArray.pos, fullArray.length, fullArray[27]);
function putArray(o: any, value: number) { o.rangeEnd = value; }
for (let i = 0; i < 24; i++) { const o: any = []; putArray(o, i); }
let arraySetter = 0;
Object.defineProperty(Array.prototype, "rangeEnd", { set(v) { arraySetter += v; }, configurable: true });
const changedArray: any = []; putArray(changedArray, 8);
console.log("array-setter", arraySetter, Object.hasOwn(changedArray, "rangeEnd"));
Object.defineProperty(Array.prototype, "rangeEnd", { value: 17, writable: false, configurable: true });
const readonlyArray: any = [];
console.log("array-readonly", Reflect.set(readonlyArray, "rangeEnd", 1), readonlyArray.rangeEnd, Object.hasOwn(readonlyArray, "rangeEnd"));
delete (Array.prototype as any).rangeEnd;
const arrayParent = Object.getPrototypeOf(Array.prototype);
let arrayProxyTotal = 0;
const arrayProxyParent = new Proxy(arrayParent, { set(_target, key, value, receiver) {
  arrayProxyTotal += value; Object.defineProperty(receiver, key, { value: value + 1, writable: true }); return true;
} });
Object.setPrototypeOf(Array.prototype, arrayProxyParent);
const proxyArray: any = []; putArray(proxyArray, 7);
Object.setPrototypeOf(Array.prototype, arrayParent);
console.log("array-proxy", arrayProxyTotal, proxyArray.rangeEnd);
let arrayGrandparentSetter = 0;
Object.defineProperty(Object.prototype, "rangeEnd", { set(v) { arrayGrandparentSetter += v; }, configurable: true });
const grandparentArray: any = []; putArray(grandparentArray, 8);
console.log("array-grandparent-setter", arrayGrandparentSetter, Object.hasOwn(grandparentArray, "rangeEnd"));
Object.defineProperty(Object.prototype, "rangeEnd", { value: 21, writable: false, configurable: true });
const grandparentReadonlyArray: any = [];
console.log("array-grandparent-readonly", Reflect.set(grandparentReadonlyArray, "rangeEnd", 1), grandparentReadonlyArray.rangeEnd, Object.hasOwn(grandparentReadonlyArray, "rangeEnd"));
delete (Object.prototype as any).rangeEnd;
function sample(x: number, y: number) { return x + y; }
const fn: any = sample; fn.pos = 1; fn.pos = 2;
fn.end = 3; fn.transformFlags = 0;
console.log("function-expandos", fn.pos, fn.end, fn.transformFlags);
Object.defineProperty(fn, "end", { writable: false });
console.log("function-own-readonly", Reflect.set(fn, "end", 4), fn.end);
function readFunctionExpando(o: any) { return o.marker; }
function markedArray(n: number): any {
  const value: any = []; value.pos = 0; value.end = 0;
  value.marker = () => n;
  return value;
}
const markedA = markedArray(1), markedB = markedArray(2);
let arrayMarkerTotal = 0;
for (let i = 0; i < 24; i++) {
  arrayMarkerTotal += readFunctionExpando(markedA)() + readFunctionExpando(markedB)();
}
console.log("array-function-spill", arrayMarkerTotal, readFunctionExpando(markedA) === readFunctionExpando(markedB));
fn.marker = function marker() { return 17; };
let markerTotal = 0;
for (let i = 0; i < 24; i++) markerTotal += readFunctionExpando(fn)();
console.log("function-read", markerTotal, readFunctionExpando(fn) === readFunctionExpando(fn));
console.log("function", fn.pos, fn.name, fn.length, typeof fn.prototype);
console.log("intrinsics", Reflect.set(fn, "name", "other"), Reflect.set(fn, "length", 9), Reflect.set(fn, "prototype", { mark: 1 }), fn.prototype.mark);
Object.preventExtensions(fn);
console.log("fn-noextend", Reflect.set(fn, "pos", 3), Reflect.set(fn, "extra", 1), fn.pos);
Object.freeze(fn);
console.log("fn-frozen", Reflect.set(fn, "pos", 4), Reflect.set(fn, "prototype", {}), fn.pos);
function freshFunction(): any { return function fresh() {}; }
function putFunction(o: any, value: number) { o.rangeEnd = value; }
for (let i = 0; i < 24; i++) putFunction(freshFunction(), i);
let functionSetter = 0;
Object.defineProperty(Function.prototype, "rangeEnd", { set(v) { functionSetter += v; }, configurable: true });
const changedFunction = freshFunction(); putFunction(changedFunction, 6);
console.log("function-setter", functionSetter, Object.hasOwn(changedFunction, "rangeEnd"));
Object.defineProperty(Function.prototype, "rangeEnd", { value: 19, writable: false, configurable: true });
const readonlyFunction = freshFunction();
console.log("function-readonly", Reflect.set(readonlyFunction, "rangeEnd", 1), readonlyFunction.rangeEnd, Object.hasOwn(readonlyFunction, "rangeEnd"));
delete (Function.prototype as any).rangeEnd;
const customFunction = freshFunction();
Object.setPrototypeOf(customFunction, proxyProto);
putFunction(customFunction, 4);
console.log("function-proxy", proxyTotal, customFunction.rangeEnd);
const wide: any = {};
for (let i = 0; i < 1030; i++) wide["shapeColumn" + i] = { value: i };
wide.shapeColumn1024 = { value: 2048 };
console.log("dictionary", Object.keys(wide).length, wide.shapeColumn0.value,
  wide.shapeColumn1023.value, wide.shapeColumn1024.value, wide.shapeColumn1029.value);
const dictionaryPrototype: any = {};
for (let i = 0; i < 1030; i++) dictionaryPrototype["protoColumn" + i] = i;
const dictionaryChild: any = Object.create(dictionaryPrototype);
for (let i = 0; i < 24; i++) Reflect.set(dictionaryChild, "warmRange", i);
Object.defineProperty(dictionaryPrototype, "protoColumn1024", { writable: false });
console.log("dictionary-proto-readonly", Reflect.set(dictionaryChild, "protoColumn1024", 7),
  dictionaryChild.protoColumn1024, Object.hasOwn(dictionaryChild, "protoColumn1024"));
let dictionarySetter = 0;
Object.defineProperty(dictionaryPrototype, "protoColumn1025", {
  set(v) { dictionarySetter += v; }, configurable: true
});
console.log("dictionary-proto-setter", Reflect.set(dictionaryChild, "protoColumn1025", 6),
  dictionarySetter, Object.hasOwn(dictionaryChild, "protoColumn1025"));
