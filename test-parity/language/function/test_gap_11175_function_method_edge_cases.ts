"use strict";
const call = Function.prototype.call;
const apply = Function.prototype.apply;
function detached() {}
Object.setPrototypeOf(detached, null);
console.log('null-proto', detached.call === undefined, detached.apply === undefined,
  detached.bind === undefined, Reflect.get(detached, 'call') === undefined);
const custom = Object.create(null);
Object.setPrototypeOf(detached, custom);
console.log('custom-proto', detached.call === undefined, detached.apply === undefined);
custom.call = 123;
console.log('custom-own', (detached as any).call, Reflect.get(detached, 'call'));
(detached as any).call = 456;
console.log('function-own', (detached as any).call);
delete (detached as any).call;
delete custom.call;
Object.setPrototypeOf(custom, Function.prototype);
console.log('restored-chain', detached.call === call, detached.apply === apply);

let trapCalls = 0;
const notCallable = new Proxy({}, { apply() { trapCalls++; return 99; } });
for (const mode of ['call', 'apply']) {
  let rejected = false;
  try {
    if (mode === 'call') call.call(notCallable, null);
    else apply.call(notCallable, null, []);
  } catch (e) { rejected = e instanceof TypeError; }
  console.log('noncallable', mode, rejected, trapCalls);
}
let argumentReads = 0;
try {
  apply.call(notCallable, null, { get length() { argumentReads++; return 0; } });
} catch (_) {}
console.log('callability-first', argumentReads, trapCalls);
const receiver = { base: 10 };
function add(this: any, a: number, b: number) { return this.base + a + b; }
const events: string[] = [];
const arrayLike = {
  get length() { events.push('length'); return 2; },
  get 0() { events.push('0'); return 3; },
  get 1() { events.push('1'); return 4; },
};
const proxy = new Proxy(add, {
  apply(target, thisArg, args) {
    events.push('trap');
    console.log('trap-array', Array.isArray(args), args === arrayLike);
    return Reflect.apply(target, thisArg, args);
  }
});
console.log('array-like', apply.call(proxy, receiver, arrayLike), events.join(','));
const forwarding = new Proxy(add, {});
console.log('forward', apply.call(forwarding, receiver, { 0: 5, 1: 6, length: 2 }));
let primitiveRejected = false;
try { apply.call(proxy, receiver, 42); }
catch (e) { primitiveRejected = e instanceof TypeError; }
console.log('primitive', primitiveRejected);
const input = [7, 8];
const copying = new Proxy(add, {
  apply(target, thisArg, args) {
    console.log('copied', args !== input);
    args[0] = 100;
    return Reflect.apply(target, thisArg, args);
  }
});
console.log('copy-result', apply.call(copying, receiver, input), input[0]);

let reflectiveRejected = false;
try { Reflect.apply(call, notCallable, [null]); }
catch (e) { reflectiveRejected = e instanceof TypeError; }
console.log('reflective-call', reflectiveRejected, trapCalls);
events.length = 0;
console.log('reflective-apply', Reflect.apply(apply, proxy, [receiver, arrayLike]), events.join(','));

const indexedReads: string[] = [];
const wrappedArgs = new Proxy([3, 4], {
  get(target, key, receiver) {
    indexedReads.push(String(key));
    return key === '0' ? 30 : Reflect.get(target, key, receiver);
  }
});
console.log('proxy-argument-array', apply.call(proxy, receiver, wrappedArgs), indexedReads.join(','));
indexedReads.length = 0;
console.log('reflective-proxy-array', Reflect.apply(apply, proxy, [receiver, wrappedArgs]), indexedReads.join(','));
indexedReads.length = 0;
console.log('direct-proxy-array', proxy.apply(receiver, wrappedArgs), indexedReads.join(','));
indexedReads.length = 0;
console.log('ordinary-proxy-array', apply.call(add, receiver, wrappedArgs), indexedReads.join(','));
console.log('nested-proxy-array', apply.call(add, receiver, new Proxy(new Proxy([3, 4], {}), {})));
const revokedArgs = Proxy.revocable([3, 4], {});
revokedArgs.revoke();
let revokedRejected = false;
try { apply.call(proxy, receiver, revokedArgs.proxy); }
catch (e) { revokedRejected = e instanceof TypeError; }
console.log('revoked-proxy-array', revokedRejected);
