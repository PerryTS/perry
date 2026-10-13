// GetPrototypeFromConstructor may collect while the fresh receiver is live.
declare function gc(): void;
function collect() { if (typeof gc === 'function') { gc(); gc(); } }
function Target(this: any, value: any) { this.payload = value; }
function NewTarget() {}
const proxy = new Proxy(NewTarget, {
  get(target, key, receiver) {
    if (key === 'prototype') collect();
    return Reflect.get(target, key, receiver);
  }
});
const payload = { n: 17 };
let result: any;
for (let i = 0; i < 12; i++) result = Reflect.construct(Target, [payload], proxy);
collect();
console.log('newTarget gc', result.payload === payload, result.payload.n, Object.getPrototypeOf(result) === proxy.prototype, result instanceof proxy, !(result instanceof Target));
