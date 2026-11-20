// Warm stores must follow the actual prototype, including changes after birth.
function F(this: any, n: number) {
  this.a = n; this.b = n + 1; this.c = n + 2; this.d = n + 3;
  this.e = n + 4; this.f = n + 5; this.g = n + 6; this.h = n + 7;
  this.i = n + 8; this.j = n + 9;
}
function make(n: number): any { return new (F as any)(n); }
let checksum = 0;
for (let n = 0; n < 30; n++) checksum += make(n).j;
const old = make(40);
const original = F.prototype;
let calls = 0;
const ancestor: any = {};
Object.defineProperty(ancestor, 'j', {
  set(this: any, v: number) { calls++; this.seen = v; },
  get(this: any) { return this.seen; }, configurable: true
});
const replacement: any = Object.create(ancestor);
Object.preventExtensions(replacement);
(F as any).prototype = replacement;
const changed = make(50);
console.log('reassigned', checksum, calls, changed.j, old.j,
  Object.getPrototypeOf(old) === original,
  Object.getPrototypeOf(changed) === replacement, old instanceof F, changed instanceof F);
Object.defineProperty(ancestor, 'j', { value: 123, writable: false, configurable: true });
let rejected = false;
try { make(60); } catch (e) { rejected = e instanceof TypeError; }
console.log('readonly inherited', rejected, old.j);
(F as any).prototype = original;
const restored = make(70);
console.log('restored', restored.j, restored instanceof F, changed instanceof F);
function G(this: any) { this.k = 8; }
const frozen = Object.freeze({ inherited: 9 });
(G as any).prototype = frozen;
const fresh: any = new (G as any)();
console.log('frozen prototype', fresh.k, fresh.inherited, Object.getPrototypeOf(fresh) === frozen);
class C { value = 3; set answer(v: number) { this.value = v + 1; } }
const c = new C(); c.answer = 10;
console.log('class', c.value, c instanceof C, c instanceof F);
// Getters added after a shape plan was warm must invalidate the negative vet.
Object.defineProperty(original, 'j', { set(this: any, v: number) { this.seen = v * 2; },
  get(this: any) { return this.seen; }, configurable: true });
const afterAccessor = make(80);
console.log('late accessor', afterAccessor.j, Object.hasOwn(afterAccessor, 'j'));
const gc = (globalThis as any).gc;
if (typeof gc === 'function') gc();
console.log('after gc', afterAccessor.j, old.j, c.value, fresh.k);
