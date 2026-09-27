class A { x = 1; }
const a: any = new A();
function churn() {
  // Keep allocation-capable loop polls inside traps for the GC-stress arm.
  for (let i = 0; i < 6; i++) globalThis.__proxyInstanceofKeep = { i, values: [i] };
}
function check(label: string, value: any, constructor: any) {
  try { console.log(label, value instanceof constructor); }
  catch (error) { console.log(label, error instanceof TypeError ? 'TypeError' : 'other'); }
}
check('object', a, new Proxy({}, {}));
check('function', a, new Proxy(function () {}, {}));
check('class', a, new Proxy(class B {}, {}));
const wrapped = new Proxy(A, {});
check('matching', a, wrapped);
check('nested', a, new Proxy(wrapped, {}));
function makeClass(n: number) { return class Captured { x = n; }; }
const C = makeClass(4);
const D = makeClass(5);
const captured = new C();
const wrappedCaptured = new Proxy(C, {});
console.log('captured callable', typeof wrappedCaptured);
check('captured matching', captured, wrappedCaptured);
check('captured sibling', captured, new Proxy(D, {}));
check('array', [], new Proxy(Array, {}));
check('arrow', a, new Proxy(() => {}, {}));
check('bound', a, new Proxy(A.bind(null), {}));

let custom: any;
custom = new Proxy({}, {
  get(target, key) {
    if (key === Symbol.hasInstance) {
      return function (value: any) {
        console.log('hook receiver', this === custom, value === a);
        return 'truthy';
      };
    }
    return Reflect.get(target, key);
  }
});
check('custom', a, custom);
check('noncallable hook', a, new Proxy({}, { get: () => 1 }));
const callableHook = new Proxy(function () { return 0; }, {
  apply(target, receiver, args) { console.log('apply hook', receiver === hooked, args[0] === a); return true; }
});
const hooked = new Proxy({}, { get: () => callableHook });
check('proxy hook', a, hooked);

const reads: string[] = [];
const redirected = new Proxy(function () {}, {
  get(target, key) {
    churn();
    reads.push(key === Symbol.hasInstance ? 'hasInstance' : String(key));
    if (key === Symbol.hasInstance) return undefined;
    if (key === 'prototype') return A.prototype;
    return Reflect.get(target, key);
  }
});
check('redirected', a, redirected);
console.log('get order', reads.join(','));
reads.length = 0;
check('primitive', 7, redirected);
console.log('primitive gets', reads.join(','));
reads.length = 0;
check('symbol', Symbol('x'), redirected);
console.log('symbol gets', reads.join(','));
const invalidPrototype = new Proxy(function () {}, {
  get(target, key) { return key === Symbol.hasInstance ? undefined : 7; }
});
check('invalid prototype', a, invalidPrototype);
check('primitive invalid prototype', 7, invalidPrototype);

const left = new Proxy({}, { getPrototypeOf() { churn(); console.log('left prototype'); return A.prototype; } });
check('left trap', left, wrapped);
const revoked = Proxy.revocable(A, {});
revoked.revoke();
check('revoked', a, revoked.proxy);
check('revoked primitive', 7, revoked.proxy);
console.log('ordinary revoked primitive', Function.prototype[Symbol.hasInstance].call(revoked.proxy, 7));
console.log('ordinary noncallable', Function.prototype[Symbol.hasInstance].call(custom, a));
console.log('ordinary callable', Function.prototype[Symbol.hasInstance].call(wrapped, a));
check('throwing hook', a, new Proxy({}, { get() { return function () { throw new TypeError('hook'); }; } }));
check('after throw', a, wrapped);
