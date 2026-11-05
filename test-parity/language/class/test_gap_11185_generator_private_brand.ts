function make() {
  return class {
    #v: number;
    static #s = 5;
    static *stat() { yield this.#s; }
    static *readStatic(other: any) { yield other.#s; }
    static makeIterator(other: any) { return other.gen(); }
    async *agen() { await Promise.resolve(); yield this.#v; }
    constructor(v: number) { this.#v = v; }
    *gen() { yield this.#v; yield this.#v + 1; }
    *[Symbol.iterator]() { yield this.#v; }
    *check(other: any) { yield (#v in other); yield other.#v; }
    *cleanup() { try { yield 'ready'; } finally { yield this.#v; } }
    *recover() { try { yield 'ready'; } catch (_) { yield this.#v; } }
    get() { return this.#v; }
    resume(g: any) { const value = g.next().value; return value + this.#v; }
    catchResume(g: any) { try { g.next(); } catch (_) { return this.#v; } return -1; }
  };
}
const A = make(), B = make();
const a = new A(3), b = new B(9);
function test(label: string, f: () => any) {
  try { console.log(label, f()); }
  catch (e: any) { console.log(label, 'threw', e.constructor.name); }
}
test('same', () => A.prototype.gen.call(a).next().value);
test('cross', () => A.prototype.gen.call(b).next().value);
test('ordinary-cross', () => A.prototype.get.call(b));
test('symbol-cross', () => A.prototype[Symbol.iterator].call(b).next().value);
const check = A.prototype.check.call(a, b);
test('brand-in', () => check.next().value);
test('other-cross', () => check.next().value);
const left = A.prototype.gen.call(a), right = B.prototype.gen.call(b);
test('interleave-a1', () => left.next().value);
test('interleave-b1', () => right.next().value);
test('interleave-a2', () => left.next().value);
test('interleave-b2', () => right.next().value);
const cleanup = A.prototype.cleanup.call(b);
test('cleanup-start', () => cleanup.next().value);
test('cleanup-return-cross', () => cleanup.return(0).value);
const recover = A.prototype.recover.call(b);
test('recover-start', () => recover.next().value);
test('recover-throw-cross', () => recover.throw(0).value);
test('after-errors', () => B.prototype.gen.call(b).next().value);

test('nested-resume', () => b.resume(A.prototype.gen.call(a)));
test('nested-throw-restores', () => b.catchResume(A.prototype.gen.call(b)));
test('static-same', () => A.stat().next().value);
test('static-cross', () => A.stat.call(B).next().value);
test('static-owner', () => A.readStatic.call(B, A).next().value);
test('static-other-cross', () => A.readStatic.call(A, B).next().value);
test('instance-from-static', () => A.makeIterator(b).next().value);
const delayed = A.prototype.gen.call(b);
console.log('creation-does-not-read-private', true);
test('delayed-cross', () => delayed.next().value);
async function asyncChecks() {
  try { console.log('async-same', (await A.prototype.agen.call(a).next()).value); }
  catch (e: any) { console.log('async-same', 'threw', e.constructor.name); }
  try { console.log('async-cross', (await A.prototype.agen.call(b).next()).value); }
  catch (e: any) { console.log('async-cross', 'threw', e.constructor.name); }
}
asyncChecks();

// Lexical ownership survives changes to the public prototype.constructor.
const C = make(), D = make();
const c = new C(12), d = new D(24);
C.prototype.constructor = D;
test('changed-constructor-same', () => C.prototype[Symbol.iterator].call(c).next().value);
test('changed-constructor-cross', () => C.prototype[Symbol.iterator].call(d).next().value);
delete C.prototype.constructor;
test('deleted-constructor-same', () => C.prototype[Symbol.iterator].call(c).next().value);
test('deleted-constructor-cross', () => C.prototype[Symbol.iterator].call(d).next().value);
const protoCheck = C.prototype.check.call(c, C.prototype);
test('prototype-not-instance', () => protoCheck.next().value);
test('prototype-private-read', () => protoCheck.next().value);
console.log('prototype-keys', Reflect.ownKeys(C.prototype).map(key => String(key)).sort().join(','));
