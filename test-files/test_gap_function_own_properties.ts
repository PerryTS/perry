const methodObject = { method() { return this; }, ['computed']() { return this; } };
for (const name of ['method', 'computed']) {
  const method: any = (methodObject as any)[name];
  console.log(name, Object.hasOwn(method, 'prototype'), method.call(methodObject) === methodObject);
}
const ctor: any = Object;
for (const name of ['hasOwnProperty', 'isPrototypeOf', 'propertyIsEnumerable', 'toString', 'toLocaleString', 'valueOf']) {
  console.log('Object own', name, Object.hasOwn(ctor, name));
}
console.log('Object inheritance', ctor.hasOwnProperty === Object.prototype.hasOwnProperty,
  ctor.toString === Function.prototype.toString, ctor.valueOf === Object.prototype.valueOf);
try { console.log('Function.prototype result', (Function.prototype as any)() === undefined); }
catch (e: any) { console.log('Function.prototype error', e.name); }
const fp: any = Function.prototype;
try { console.log('indirect prototype result', fp(1, 2) === undefined); }
catch (e: any) { console.log('indirect prototype error', e.name); }
const accessors = Object.getOwnPropertyDescriptor({ get x() { return 1; }, set x(value: number) {} }, 'x')!;
console.log('accessors', Object.hasOwn(accessors.get!, 'prototype'), Object.hasOwn(accessors.set!, 'prototype'));
const method: any = methodObject.method;
try { new method(); console.log('method constructed'); }
catch (e: any) { console.log('method construct error', e.name); }
const other: any = { f: function () {}, a: () => {}, *g() { yield 1; } };
console.log('other forms', Object.hasOwn(other.f, 'prototype'), Object.hasOwn(other.a, 'prototype'), Object.hasOwn(other.g, 'prototype'));
try { new (methodObject.method as any)(); console.log('direct method constructed'); }
catch (e: any) { console.log('direct method construct error', e.name); }
