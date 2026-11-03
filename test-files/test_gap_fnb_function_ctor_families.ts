const a = Symbol('same'), b = Symbol('same'), reg = Symbol.for('FNB');
const names = (o: any) => Object.keys(o).join(',') + '|' + Object.getOwnPropertySymbols(o).map(s => s === a ? 'a' : s === b ? 'b' : s === reg ? 'reg' : '?').join(',');
function F(this: any, n: number) { this.x = n; this[a] = n + 1; this[b] = n + 2; this[reg] = n + 3; this.nt = new.target === F; }
const p = F.prototype, f = new F(1);
F.prototype = { marker: 2 };
const g = new F(4);
console.log('proto', Object.getPrototypeOf(f) === p, Object.getPrototypeOf(g) === F.prototype, f.x, g.x, f.nt, g.nt, names(f));
function R(this: any) { this.a = 1; this[b] = 2; return { result: 3 }; }
console.log('return', (new R() as any).result);
function C(this: any, yes: boolean) { this.first = 1; if (yes) this[a] = 2; this.last = 3; }
console.log('conditional', names(new C(false)), names(new C(true)));
delete f[a]; f[a] = 9;
console.log('reinsert', names(f), f[a]);
Object.defineProperty(f, b, { get() { return 17; }, configurable: true });
console.log('redefine', f[b], names(f));
const borrowed: any = {}; F.call(borrowed, 20); console.log('borrow', borrowed.x, borrowed[a], borrowed.nt, names(borrowed));
function Setter(this: any) { this[a] = 1; this[b] = 2; }
Object.defineProperty(Setter.prototype, a, { set(v) { this.got = v + 10; }, configurable: true });
const ss = new Setter(); console.log('setter', ss.got, Object.hasOwn(ss, a), ss[b], names(ss));
let effects = 0;
function Mut(this: any) { this.x = (++effects, Object.defineProperty(Mut.prototype, a, { set(v) { this.got = v; }, configurable: true }), 1); this[a] = 19; }
const mm = new Mut(); console.log('rhs-mutation', effects, mm.x, mm.got, Object.hasOwn(mm, a));
function Early(this: any) { this.x = (++effects, 1); this[late] = 8; }
try { new Early(); } catch(e) { console.log('tdz', effects, e instanceof ReferenceError); }
const late = Symbol('late');
console.log('late', (new Early())[late]);
function factory(k: symbol) { return function Same(this: any) { this.x = 2; this[k] = 3; }; }
const A = factory(a), B = factory(b); const aa = new A(), bb = new B();
console.log('factory', aa[a], aa[b], bb[a], bb[b], Object.getPrototypeOf(aa) === A.prototype, Object.getPrototypeOf(bb) === B.prototype);
function Order(this: any) { this.x = (this[b] = 12, 1); this[a] = 3; this[b] = 4; }
console.log('rhs-order', names(new Order()));
function Freeze(this: any) { this.x = (Object.preventExtensions(this), 1); this[a] = 4; }
try { new Freeze(); } catch(e) { console.log('noextend', e instanceof TypeError); }
let escaped: any;
function ThrowCtor(this: any) { this.x = (escaped = this, 1); this[a] = (() => { throw new Error("stop"); })(); this.last = 3; }
try { new ThrowCtor(); } catch(e) { console.log("partial", names(escaped), Object.hasOwn(escaped, a), escaped.x); }
function Observe(this: any) { this.x = (console.log("before-store", names(this)), 1); this[a] = 2; }
console.log("after-store", names(new Observe()));
function OnlySymbol(this: any) { this[a] = (++effects, 1); }
const beforeNull = effects;
try { OnlySymbol.call(null); } catch(e) { console.log("borrow-null", effects === beforeNull, e instanceof TypeError); }
let dynamicKey: any = { [Symbol.toPrimitive]() { console.log("coerce-key"); return a; } };
function Coerce(this: any) { this[dynamicKey] = (console.log("coerce-rhs"), 7); }
console.log("coerce-value", (new Coerce())[a]);
function FreezeSymbol(this: any) { this[a] = (Object.preventExtensions(this), 1); }
try { new FreezeSymbol(); } catch(e) { console.log("frozen-symbol", e instanceof TypeError); }
let getterCalls = 0;
function GetterOnly(this: any) { this[a] = 3; }
Object.defineProperty(GetterOnly.prototype, a, { get() { getterCalls++; return 4; }, configurable: true });
try { new GetterOnly(); } catch(e) { console.log("getter-only", e instanceof TypeError, getterCalls); }
function scopedFactory() {
  let key: any = a;
  const Body = function(this: any) { this.first = 1; this[key] = 2; this.last = 3; };
  return { Body, change() { key = b; } };
}
const scoped = scopedFactory();
const scopedA = new scoped.Body(); scoped.change(); const scopedB = new scoped.Body();
console.log('scope-symbol', scopedA[a], scopedA[b], scopedB[a], scopedB[b], names(scopedA), names(scopedB));
function lateFactory() {
  function Body(this: any) { this.first = (++effects, 1); this[key] = 2; }
  try { new Body(); } catch (e) { console.log('scope-tdz', e instanceof ReferenceError); }
  const key = Symbol('inner');
  return new Body();
}
console.log('scope-ready', Object.getOwnPropertySymbols(lateFactory()).length);
