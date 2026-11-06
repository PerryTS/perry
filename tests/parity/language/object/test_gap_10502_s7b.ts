function F() {}
F.prototype.m = function () { return 'old'; };
const old: any = new F();
function invoke(x: any) { return x.m(); }
for (let i = 0; i < 100; ++i) invoke(old);
F.prototype = { m() { return 'new'; } };
const fresh: any = new F();
console.log('replacement', invoke(old), invoke(fresh));

class Lazy { m() { return 7; } }
const lazy: any = new Lazy();
function read(x: any) { return x.extra; }
console.log('before', read(lazy));
const holder: any = Lazy.prototype;
holder.extra = 9;
console.log('after', read(lazy), lazy.m());
const detached: any = new Lazy();
Object.setPrototypeOf(detached, null);
console.log('detached', typeof detached.m);

class Probe { value = 4; }
const probe: any = new Probe();
console.log('json before', JSON.stringify(probe));
const pp: any = Probe.prototype;
pp.toJSON = function () { return 'patched'; };
console.log('json after', JSON.stringify(probe));
async function thenProbe() {
    const before = await probe;
    console.log('then before', before.value);
    pp.then = function (resolve: any) { resolve(12); };
    console.log('then after', await probe);
}
class Static { static m() { return 1; } }
function stat() { return Static.m(); }
for (let i = 0; i < 100; ++i) stat();
console.log('static before', stat());
Static.m = function () { return 2; };
console.log('static after', stat());

// An object literal's shape id is not a class identity: no declaration
// holder, its constructor is Object's, with or without literal methods.
function ctorOf(x: any) { return x.constructor; }
const plain: any = { a: 1 };
const withMethod: any = { a: 1, f() { return this.a; } };
const withGetter: any = { get z() { return 3; }, b: 2 };
let literalCtors = 0;
for (let i = 0; i < 50; ++i) {
    const fresh: any = { k: i, h() { return i; } };
    if (ctorOf(plain) === Object && ctorOf(withMethod) === Object && ctorOf(fresh) === Object) literalCtors++;
}
console.log('literal ctor', literalCtors, ctorOf(withGetter) === Object, withMethod.f(), withGetter.z);
console.log('literal proto', Object.getPrototypeOf(withMethod) === Object.prototype,
    Object.getOwnPropertyNames(withMethod).join(','), typeof plain.constructor.isBuffer);
const Anon = [class { q() { return 7; } }][0];
const anon: any = new Anon();
console.log('anon class', anon.constructor === Anon, anon.q(), Object.getPrototypeOf(anon) === Anon.prototype);

// Default heritage goes through the production class-evaluation entry: each
// captured evaluation owns its prototype, including after deletion/redefine.
function capturedClass(tag: string) { return class { m() { return tag; } }; }
const e1: any = capturedClass('one'), e2: any = capturedClass('two'), e3: any = capturedClass('three');
console.log('eval default', Object.getPrototypeOf(e2.prototype) === Object.prototype,
    new e1().m(), new e2().m(), new e3().m());
delete e2.prototype.m;
let deletedMethodThrows = false;
try { new e2().m(); } catch (e) { deletedMethodThrows = e instanceof TypeError; }
console.log('eval delete', typeof e2.prototype.m, deletedMethodThrows, new e1().m(), new e3().m());
e2.prototype.m = function () { return 'again'; };
console.log('eval redefine', new e2().m(), new e1().m(), new e3().m());
thenProbe();
