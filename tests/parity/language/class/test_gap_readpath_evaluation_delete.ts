function factory(tag: string) { return class { m() { return tag; } }; }
function invoke(o: any) { return o.m(); }
function computed(o: any, k: string) { return o[k](); }
const A: any = factory('a'), B: any = factory('b');
const a: any = new A(), b: any = new B();
for (let i = 0; i < 100; i++) { invoke(a); invoke(b); computed(b, 'm'); }
const saved = b.m;
console.log('before', invoke(a), invoke(b), saved.call(b));
delete B.prototype.m;
console.log('reflect', typeof b.m, 'm' in b, typeof a.m);
for (const o of [b, new B()]) {
    try { console.log('named', invoke(o)); } catch (e: any) { console.log('named', e instanceof TypeError); }
    try { console.log('computed', computed(o, 'm')); } catch (e: any) { console.log('computed', e instanceof TypeError); }
}
console.log('saved', saved.call(b), invoke(a));
B.prototype.m = function () { return 'replacement'; };
console.log('replaced', invoke(b), computed(new B(), 'm'), invoke(a));
function plainFactory() { return class { m() { return 7; } }; }
const P: any = plainFactory(), p: any = new P();
invoke(p); delete P.prototype.m;
try { console.log('plain', invoke(p)); } catch (e: any) { console.log('plain', e instanceof TypeError); }
