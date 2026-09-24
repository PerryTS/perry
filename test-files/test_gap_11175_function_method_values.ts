"use strict";
console.log(Function.call === Function.prototype.call, typeof Function.call, Function.call.name);
function g(this: any, a: any) { return [typeof this, a]; }
const fc: any = (Function as any).call;
console.log(JSON.stringify(fc.bind(g)('T', 6)));
const oc: any = (Object as any).call;
console.log(oc === Function.prototype.call, JSON.stringify(oc.bind(g)('T', 7)));
const hasInst = Function.call.bind(Function.prototype[Symbol.hasInstance]);
console.log(typeof hasInst, hasInst(Array, []), hasInst(Array, {}));
for (const [label, fn] of [['Function', Function], ['Object', Object], ['Array', Array], ['user', g]] as any[]) {
  console.log('identity', label, fn.call === Function.prototype.call,
    fn.apply === Function.prototype.apply, fn.bind === Function.prototype.bind);
}
function add(this: any, n: number) { return this.base + n; }
const receiver = { base: 20 };
const call: any = Object.call;
const apply: any = Object.apply;
const bind: any = Object.bind;
console.log('borrowed', call.call(add, receiver, 1), apply.call(add, receiver, [2]), bind.call(add, receiver, 3)());
console.log('uncurry', Function.call.bind(add)(receiver, 4));
const savedCall = Function.prototype.call;
const replacement: any = function () { return 99; };
(Function.prototype as any).call = replacement;
const inheritedReplacement = Object.call === replacement;
(Function.prototype as any).call = savedCall;
console.log('patched', inheritedReplacement);
Object.defineProperty(Function.prototype, 'call', {
  configurable: true, get() { return this === Object ? replacement : savedCall; },
});
const getterReceiver = Object.call === replacement;
Object.defineProperty(Function.prototype, 'call', {
  configurable: true, writable: true, value: savedCall,
});
console.log('getter-receiver', getterReceiver);
(add as any).call = replacement;
console.log('own', (add as any).call === replacement);
delete (add as any).call;
console.log('restored', add.call === savedCall);
console.log('reflect', Reflect.get(Function, 'call') === savedCall,
  Reflect.get(Object, 'apply') === Function.prototype.apply,
  Reflect.get(Array, 'bind') === Function.prototype.bind);
const proxied = new Proxy(add, {
  apply(target, thisArg, args) { return Reflect.apply(target, thisArg, args) + 100; },
});
const proxyCall = proxied.call;
console.log('proxy', proxyCall === savedCall, proxyCall.call(proxied, receiver, 5));

let reads = 0;
Object.defineProperty(Function.prototype, 'call', {
  configurable: true, get() { reads++; return undefined; },
});
const missingCall = Object.call;
Object.defineProperty(Function.prototype, 'call', {
  configurable: true, writable: true, value: savedCall,
});
console.log('undefined-getter', missingCall === undefined, reads);
(add as any).call = undefined;
console.log('own-undefined', (add as any).call === undefined);
delete (add as any).call;

const proxyApply = proxied.apply;
console.log('proxy-apply', proxyApply === Function.prototype.apply,
  proxyApply.call(proxied, receiver, [6]));
const zero = new Proxy(function(this: any) { return this.base; }, {});
console.log('proxy-null-args', zero.apply.call(zero, receiver, null));
class Example {}
const Klass: any = Example;
console.log('class-methods', Klass.call === Function.prototype.call,
  Klass.apply === Function.prototype.apply, Klass.bind === Function.prototype.bind);
let classCallRejected = false;
try { Klass.call(null); } catch (error) { classCallRejected = error instanceof TypeError; }
console.log('class-call-rejected', classCallRejected);
