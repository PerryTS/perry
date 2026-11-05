// Static class accessors are real accessor properties of the constructor;
// C.prototype's own names are its real own keys.
class C { static get a() { return 1; } static set a(v: number) {} static get k() { return 3; } }
console.log(Reflect.deleteProperty(C, "a"), "a" in C, Object.getOwnPropertyDescriptor(C, "a") === undefined);
Object.defineProperty(C, "b", { get() { return 2; }, configurable: false });
console.log(Reflect.deleteProperty(C, "b"), (C as any).b, Object.getOwnPropertyDescriptor(C, "b")!.configurable);
Object.defineProperty(C, "k", { configurable: false });
console.log(Reflect.deleteProperty(C, "k"), (C as any).k);
class D { m() {} get g() { return 1; } set g(v) {} static s() {} static get sg() { return 1; } static f = 1; x = 1; }
console.log(Object.getOwnPropertyNames(D.prototype).sort().join(","));
delete (D.prototype as any).m;
console.log(Object.getOwnPropertyNames(D.prototype).sort().join(","), "m" in new D());
(D.prototype as any).added = 1;
console.log(Object.getOwnPropertyNames(D.prototype).sort().join(","));
delete (D.prototype as any).g;
console.log(Object.getOwnPropertyNames(D.prototype).sort().join(","), Reflect.ownKeys(D.prototype).length);
class E extends D { n() {} }
console.log(Object.getOwnPropertyNames(E.prototype).sort().join(","), Object.getOwnPropertyNames(Object.getPrototypeOf(E.prototype)).sort().join(","));
