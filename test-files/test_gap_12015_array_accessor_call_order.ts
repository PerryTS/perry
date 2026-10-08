// Array.prototype data descriptors must not stand in for accessor history.
// Each API runs at the same effectful call site, before and after warming it.
let events = "";
let record = false;
let gets = 0;
let expectedReceiver: any;
function arg(): number {
  if (record) events += "a";
  return 7;
}
function call(arr: any): any { return arr.push(arg()); }
function getter(this: any): any {
  gets++;
  if (record) events += this === expectedReceiver ? "g" : "wrong receiver";
  return function(this: any, x: number): number {
    if (record) events += "c";
    return x + 10;
  };
}
function setter(this: any, value: any): void {}
function install(proto: any, api: number): void {
  if (api === 0) Object.defineProperty(proto, "push", { configurable: true, get: getter });
  if (api === 1) proto.__defineGetter__("push", getter);
  if (api === 2) {
    proto.__defineSetter__("push", setter);
    proto.__defineGetter__("push", getter);
  }
  if (api === 3) Object.defineProperties(proto, { push: { configurable: true, get: getter } });
  if (api === 4) Reflect.defineProperty(proto, "push", { configurable: true, get: getter });
  if (api === 5) {
    const methods = { get push() { return getter.call(this); }, set push(v: any) {} };
    Object.defineProperty(proto, "push", Object.getOwnPropertyDescriptor(methods, "push")!);
  }
  if (api === 6) {
    class Methods { get push() { return getter.call(this); } set push(v: any) {} }
    Object.defineProperties(proto, { push: Object.getOwnPropertyDescriptor(Methods.prototype, "push")! });
  }
}
const arrayProto: any = Array.prototype;
const objectProto: any = Object.prototype;
const original = Object.getOwnPropertyDescriptor(arrayProto, "push")!;
for (let owner = 0; owner < 2; owner++) {
  const proto: any = owner === 0 ? arrayProto : objectProto;
  for (let api = 0; api < 7; api++) {
    for (let timing = 0; timing < 2; timing++) {
      const arr: any = [];
      record = false;
      gets = 0;
      if (timing === 0) {
        if (owner === 1) delete arrayProto.push;
        install(proto, api);
      }
      for (let i = 0; i < 2000; i++) call(arr);
      if (timing === 1) {
        if (owner === 1) delete arrayProto.push;
        install(proto, api);
      }
      events = "";
      record = true;
      expectedReceiver = arr;
      const result = call(arr);
      record = false;
      console.log(owner, api, timing, events, result, gets);
      delete proto.push;
      Object.defineProperty(arrayProto, "push", original);
    }
  }
}
// Setter-only and empty accessor descriptors still shadow the builtin method.
for (let api = 0; api < 3; api++) {
  if (api === 0) arrayProto.__defineSetter__("push", setter);
  if (api === 1) Object.defineProperty(arrayProto, "push", { configurable: true, get: undefined });
  if (api === 2) Reflect.defineProperty(arrayProto, "push", { configurable: true, set: setter });
  events = "";
  record = true;
  try { call([]); } catch (e) { events += "t"; }
  record = false;
  console.log("empty", api, events);
  delete arrayProto.push;
  Object.defineProperty(arrayProto, "push", original);
}
// Same fact on a receiver with data descriptors, after it has grown.
const own: any = [];
Object.defineProperty(own, "data", { configurable: true, value: 3 });
for (let i = 0; i < 2000; i++) call(own);
Object.defineProperty(own, "push", { configurable: true, get: getter });
events = "";
record = true;
expectedReceiver = own;
console.log("own", call(own), events);
record = false;
delete own.push;
events = "";
record = true;
console.log("deleted", call(own), events);
record = false;
// Array.prototype itself can grow and leave forwarding aliases behind.
const protoAlias: any = arrayProto;
Object.defineProperty(protoAlias, "protoaccGrow", { configurable: true, get: getter });
protoAlias[400] = 1;
events = "";
record = true;
const grownReceiver: any = [];
expectedReceiver = grownReceiver;
const grownResult = grownReceiver.protoaccGrow(arg());
record = false;
console.log("prototype growth", grownResult, events);
delete arrayProto.protoaccGrow;
delete arrayProto[400];
arrayProto.length = 0;
// Symbol-key accessor installs share the array fact; deletion keeps it armed.
const sym = Symbol("protoacc");
Object.defineProperty(arrayProto, sym, { configurable: true, get: getter });
console.log("symbol", typeof ([] as any)[sym]);
delete arrayProto[sym];
console.log("deleted symbol", typeof ([] as any)[sym]);
console.log("grown iterator", typeof ([] as any)[Symbol.iterator]);


// An empty own accessor shadows a getter further up the prototype chain.
Object.defineProperty(objectProto, "push", { configurable: true, get: getter });
Object.defineProperty(arrayProto, "push", { configurable: true, get: undefined });
events = "";
record = true;
try { call([]); } catch (e) { events += "t"; }
record = false;
console.log("undefined shadow", events);
delete objectProto.push;
delete arrayProto.push;
Object.defineProperty(arrayProto, "push", original);


// Rebinding the global constructor does not change an array's intrinsic chain.
const savedArray: any = Array;
Object.defineProperty(arrayProto, sym, { configurable: true, get: getter });
(globalThis as any).Array = undefined;
console.log("intrinsic symbol", typeof ([] as any)[sym]);
(globalThis as any).Array = savedArray;
delete arrayProto[sym];

// Accessor conversion retains property insertion order on arrays and their prototype.
const ordered: any = [];
ordered.protoaccFirst = 1;
ordered.protoaccSecond = 2;
Object.defineProperty(ordered, "protoaccFirst", { configurable: true, enumerable: true, get: getter });
console.log("own order", Object.keys(ordered).join(","));
arrayProto.protoaccFirst = 1;
arrayProto.protoaccSecond = 2;
Object.defineProperty(arrayProto, "protoaccFirst", { configurable: true, enumerable: true, get: getter });
console.log("prototype order", Object.keys(arrayProto).filter((k: string) => k.startsWith("protoacc")).join(","));
delete arrayProto.protoaccFirst;
delete arrayProto.protoaccSecond;

// Own named and dense data must shadow a prepared inherited getter.
Object.defineProperty(arrayProto, "push", { configurable: true, get: getter });
const shadow: any = [];
Object.defineProperty(shadow, "push", { configurable: true, value: function(x: number): number { if (record) events += "c"; return x + 10; } });
events = "";
record = true;
console.log("own data shadow", call(shadow), events);
record = false;
delete arrayProto.push;
Object.defineProperty(arrayProto, "push", original);
Object.defineProperty(arrayProto, "0", { configurable: true, get: getter });
const dense: any = [function(x: number): number { if (record) events += "c"; return x + 10; }];
events = "";
record = true;
const denseResult = dense[0](arg());
record = false;
delete arrayProto[0];
console.log("dense data shadow", denseResult, events);
