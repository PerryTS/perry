// #9365: a property named "prototype" is an ordinary property on non-functions.
function assign(target: any, value: any): any {
  return (target.prototype = value);
}
function assignStrict(target: any, value: any): any {
  "use strict";
  return (target.prototype = value);
}
function rejected(target: any): boolean {
  try {
    assignStrict(target, 99);
    return false;
  } catch (error) {
    return error instanceof TypeError;
  }
}

const payload = { marker: 7 };
const parameter: any = {};
console.log("parameter", assign(parameter, payload) === payload);
console.log("own", Object.hasOwn(parameter, "prototype"), parameter.prototype === payload);
const descriptor = Object.getOwnPropertyDescriptor(parameter, "prototype");
console.log("descriptor", descriptor.writable, descriptor.enumerable, descriptor.configurable);
console.log("keys", Object.keys(parameter).join(","));

for (let count = 0; count < 4; count++) {
  const dynamic: any = {};
  for (let i = 0; i < count; i++) dynamic["x" + i] = i;
  dynamic.prototype = payload;
  console.log("dynamic", count, Object.hasOwn(dynamic, "prototype"), dynamic.prototype === payload);
}

const primitiveValues: any[] = [17, "text", true, null, undefined];
for (const value of primitiveValues) {
  const target: any = {};
  console.log("value", assign(target, value) === value, target.prototype === value);
}
const arrayValue = [1, 2];
const arrayTarget: any = [];
console.log("array", assign(arrayTarget, arrayValue) === arrayValue, arrayTarget.prototype === arrayValue);
const computed: any = {};
computed["prototype"] = payload;
console.log("computed", computed.prototype === payload);

let setterCalls = 0;
let setterThis: any;
let setterValue: any;
const accessor: any = {};
Object.defineProperty(accessor, "prototype", {
  set(value) { setterCalls++; setterThis = this; setterValue = value; },
  configurable: true,
});
console.log("setter-result", assign(accessor, payload) === payload);
console.log("setter", setterCalls, setterThis === accessor, setterValue === payload);
const inherited: any = Object.create(accessor);
assign(inherited, arrayValue);
console.log("inherited", setterCalls, setterThis === inherited, setterValue === arrayValue,
  Object.hasOwn(inherited, "prototype"));

let proxyCalls = 0;
const proxyTarget: any = {};
let proxy: any;
proxy = new Proxy(proxyTarget, {
  set(target, key, value, receiver) {
    proxyCalls++;
    console.log("trap", key, receiver === proxy);
    return Reflect.set(target, key, value, receiver);
  },
});
console.log("proxy-result", assign(proxy, payload) === payload, proxyTarget.prototype === payload, proxyCalls);
const rejectingProxy = new Proxy({}, { set() { return false; } });
console.log("proxy-reject", assign(rejectingProxy, payload) === payload, rejected(rejectingProxy));

const readonly: any = {};
Object.defineProperty(readonly, "prototype", { value: 12, writable: false });
console.log("readonly", assign(readonly, 13), readonly.prototype, rejected(readonly));
const frozen = Object.freeze({});
console.log("frozen", assign(frozen, payload) === payload, Object.hasOwn(frozen, "prototype"), rejected(frozen));
console.log("primitive", assign(42, payload) === payload, rejected(42));
console.log("nullish", rejected(null), rejected(undefined));

let receiverCalls = 0;
let rhsCalls = 0;
let order = "";
const ordered: any = {};
function receiver(): any { receiverCalls++; order += "r"; return ordered; }
function rhs(): any { rhsCalls++; order += "v"; return payload; }
console.log("order-result", (receiver().prototype = rhs()) === payload);
console.log("order", receiverCalls, rhsCalls, order, ordered.prototype === payload);
const holder = { get target(): any { receiverCalls++; return ordered; } };
holder.target.prototype = arrayValue;
console.log("getter-once", receiverCalls, ordered.prototype === arrayValue);
(receiverCalls > 0 ? receiver() : holder.target).prototype = payload;
console.log("conditional-once", receiverCalls, ordered.prototype === payload);

function Base() {}
const basePrototype = { method() { return 23; } };
assign(Base, basePrototype);
class Derived extends Base {}
console.log("function", Base.prototype === basePrototype, new Derived().method());
const functionDescriptor = Object.getOwnPropertyDescriptor(Base, "prototype");
console.log("function-descriptor", functionDescriptor.writable, functionDescriptor.enumerable, functionDescriptor.configurable);
Object.defineProperty(Base, "prototype", { writable: false });
console.log("function-readonly", assign(Base, payload) === payload, Base.prototype === basePrototype, rejected(Base));
class StillDerived extends Base {}
console.log("function-retained", new StillDerived().method());
const arrow: any = () => 1;
assign(arrow, payload);
const arrowDescriptor = Object.getOwnPropertyDescriptor(arrow, "prototype");
console.log("arrow", arrow.prototype === payload, arrowDescriptor.writable,
  arrowDescriptor.enumerable, arrowDescriptor.configurable);

class Carrier {
  prototype() { return 10; }
  read() { return this.prototype(); }
}
const carrier = new Carrier();
console.log("class-method-before", carrier.read());
assign(Carrier.prototype, function() { return 42; });
console.log("class-method-after", carrier.read());
const carrierPrototype = Carrier.prototype;
console.log("class-readonly", assign(Carrier, payload) === payload,
  Carrier.prototype === carrierPrototype, rejected(Carrier), Reflect.set(Carrier, "prototype", payload));
