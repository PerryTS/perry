// The public protocol is inherited; private state remains independent of
// user properties, prototype replacement, and escaped lexical bindings.
function* applySource(value: number) { yield value; }
const generatorFunctionParent: any = Object.getPrototypeOf(applySource);
const functionParent = Object.getPrototypeOf(generatorFunctionParent);
function applyGenerator(fn: any, value: number) { return fn.apply(undefined, [value]).next().value; }
let applySum = 0;
for (let i = 0; i < 32; i++) applySum += applyGenerator(applySource, i);
console.log('generator-apply-hot', applySum);
generatorFunctionParent.apply = function(receiver: any, args: any[]) {
  return { next() { return { value: args[0] + 1000, done: true }; } };
};
console.log('generator-apply-shadow', applyGenerator(applySource, 3));
delete generatorFunctionParent.apply;
Object.setPrototypeOf(generatorFunctionParent, { apply() {
  return { next() { return { value: 2000, done: true }; } };
} });
console.log('generator-apply-parent', applyGenerator(applySource, 4));
Object.setPrototypeOf(generatorFunctionParent, functionParent);
console.log('generator-apply-restored', applyGenerator(applySource, 5));

function* source(n: number) {
  let local = n;
  try {
    const sent = yield local++;
    yield local + Number(sent);
    return local + 10;
  } finally {
    console.log("finally", n);
  }
}
function print(label: string, result: any) {
  console.log(label, result.value, result.done);
}
const a: any = source(2), b: any = source(5);
console.log("own", Object.getOwnPropertyNames(a).join(","));
console.log("same-next", a.next === b.next);
console.log("prototype", Object.getPrototypeOf(a) === source.prototype);
print("a1", a.next(99)); print("b1", b.next());
print("a2", a.next(4)); print("a3", a.next()); print("a4", a.next());
print("b-return", b.return(17)); print("b-done", b.next());
const gp: any = Object.getPrototypeOf(source.prototype);
const original = gp.next;
let calls = 0;
gp.next = function (value: any) { calls++; return original.call(this, value); };
const patched: any = source(8);
print("patched", patched.next());
let seen = 0;
for (const value of source(3)) { seen += value; break; }
console.log("patched-for-of", seen, calls);
gp.next = original;
patched.return();
const fake = { next() {}, return() {}, throw() {} };
try { original.call(fake); } catch (e) { console.log("brand", e instanceof TypeError); }
try { original.call(source.prototype); } catch (e) { console.log("prototype-brand", e instanceof TypeError); }
const unstarted: any = source(9);
print("unstarted-return", unstarted.return(23));
try { unstarted.throw("closed"); } catch (e) { console.log("closed-throw", e); }
function* catching() { try { yield 1; } catch (e) { yield e; } finally { yield 3; } return 4; }
const c: any = catching();
print("catch1", c.next()); print("catch2", c.throw(2)); print("catch3", c.next()); print("catch4", c.next());
const d: any = catching();
print("return1", d.next()); print("return2", d.return(7)); print("return3", d.next());
function* delegate() { const result = yield* catching(); return result + 10; }
const e: any = delegate();
print("delegate1", e.next()); print("delegate2", e.throw(6)); print("delegate3", e.next()); print("delegate4", e.next());
const methods = { base: 30, *values() { let value = this.base; yield value; return ++value; } };
const m: any = methods.values(); print("method1", m.next()); print("method2", m.next());
let read: () => number = () => -1, write: (n: number) => void = () => {};
function* captured() { let x = 40; read = () => x; write = (n: number) => { x = n; }; yield x; yield x; x++; }
const f: any = captured(); print("capture1", f.next()); write(50); print("capture2", f.next()); f.next(); console.log("escaped", read());
const expand: any = source(4); for (let i = 0; i < 20; i++) expand["key" + i] = i;
print("expando1", expand.next()); print("expando-return", expand.return(80));
const replaced: any = source(6); replaced.next = () => ({ value: "override", done: true }); print("own-next", replaced.next());
print("private-return", replaced.return(90));

class MethodOwner { base = 60; *values() { yield this.base; } }
const owner = new MethodOwner();
const classIterator: any = owner.values();
console.log("class-prototype", Object.getPrototypeOf(classIterator) === (owner.values as any).prototype);
print("class-next", classIterator.next());
const originalProto = source.prototype;
const replacement = Object.create(gp);
(source as any).prototype = replacement;
const changed: any = source(12);
console.log("replaced-prototype", Object.getPrototypeOf(changed) === replacement);
print("changed-next", changed.next()); changed.return();
(source as any).prototype = 0;
const primitive: any = source(13);
console.log("primitive-prototype", Object.getPrototypeOf(primitive) === gp);
print("primitive-next", primitive.next()); primitive.return();
(source as any).prototype = originalProto;

function makeMethodClass() { return class { *values() { yield 71; } }; }
const C1 = makeMethodClass(), C2 = makeMethodClass();
const c1 = new C1(), c2 = new C2();
console.log("fresh-methods", c1.values !== c2.values);
console.log("fresh-method-prototype", Object.getPrototypeOf(c1.values()) === (c1.values as any).prototype, Object.getPrototypeOf(c2.values()) === (c2.values as any).prototype);
console.log("array-from", Array.from(function*() { yield 2; yield 3; }()).join(","));

let getParam: () => number, setParam: (value: number) => void;
function* sharedParam(n: number) { getParam = () => n; setParam = value => { n = value; }; yield n; yield ++n; }
const param: any = sharedParam(80);
print("param1", param.next()); setParam!(90); print("param2", param.next()); console.log("param-shared", getParam!());

(source as any).prototype = undefined;
const undefinedProto: any = source(14);
console.log("undefined-prototype", Object.getPrototypeOf(undefinedProto) === gp, (source as any).prototype === undefined);
undefinedProto.return(); (source as any).prototype = originalProto;

function makeIteratorClass() { return class { *[Symbol.iterator]() { yield 101; } }; }
const IC1 = makeIteratorClass(), IC2 = makeIteratorClass();
const ic1 = new IC1(), ic2 = new IC2();
console.log("symbol-method-prototype", Object.getPrototypeOf(ic1[Symbol.iterator]()) === (ic1[Symbol.iterator] as any).prototype, Object.getPrototypeOf(ic2[Symbol.iterator]()) === (ic2[Symbol.iterator] as any).prototype);
console.log("symbol-method-for-of", Array.from(ic2).join(","));
const borrowed = owner.values;
const borrowedIterator: any = borrowed.call({ base: 102 });
console.log("borrowed-prototype", Object.getPrototypeOf(borrowedIterator) === (borrowed as any).prototype);
print("borrowed-next", borrowedIterator.next());
class StaticOwner { static base = 103; static *values() { yield this.base; } static *["computed"]() { yield 104; } }
const staticIterator: any = StaticOwner.values();
console.log("static-prototype", Object.getPrototypeOf(staticIterator) === (StaticOwner.values as any).prototype);
print("static-next", staticIterator.next());
const computedIterator: any = StaticOwner.computed();
console.log("computed-prototype", Object.getPrototypeOf(computedIterator) === (StaticOwner.computed as any).prototype);
print("computed-next", computedIterator.next());
const oldMethod = owner.values;
function replaceMethod() { (owner as any).values = function*() { yield 105; }; return 0; }
const ordered: any = (owner.values as any)(replaceMethod());
console.log("ordered-method-prototype", Object.getPrototypeOf(ordered) === (oldMethod as any).prototype);
print("ordered-method-next", ordered.next());

function makeDerived() {
  class Base { *values() { yield 106; } }
  return class extends Base { *via() {
    const it = super.values();
    console.log("super-prototype", Object.getPrototypeOf(it) === (Base.prototype.values as any).prototype);
    yield* it;
  } };
}
const Derived1 = makeDerived(), Derived2 = makeDerived();
console.log("super-values", Array.from(new Derived1().via()).join(","), Array.from(new Derived2().via()).join(","));

const arbitrary = Symbol("generator-method");
class SymbolOwner {
  *[arbitrary]() { yield 108; }
  static *[arbitrary]() { yield 109; }
  static *[Symbol.iterator]() { yield 110; }
}
const symbolOwner: any = new SymbolOwner();
const customInstance: any = symbolOwner[arbitrary]();
const customStatic: any = (SymbolOwner as any)[arbitrary]();
const staticSymbol: any = SymbolOwner[Symbol.iterator]();
console.log("custom-method-prototypes", Object.getPrototypeOf(customInstance) === symbolOwner[arbitrary].prototype, Object.getPrototypeOf(customStatic) === (SymbolOwner as any)[arbitrary].prototype);
console.log("static-symbol-prototype", Object.getPrototypeOf(staticSymbol) === (SymbolOwner[Symbol.iterator] as any).prototype);
print("custom-instance", customInstance.next()); print("custom-static", customStatic.next()); print("static-symbol", staticSymbol.next());
class StaticChild extends StaticOwner {}
const inheritedStatic: any = StaticChild.computed();
console.log("inherited-computed-prototype", Object.getPrototypeOf(inheritedStatic) === (StaticOwner.computed as any).prototype);
print("inherited-computed-next", inheritedStatic.next());

function keyedClass(key: symbol) { return class { *[key]() { yield 112; } }; }
const keyA = Symbol('a'), keyB = Symbol('b');
const KeyedA = keyedClass(keyA), KeyedB = keyedClass(keyB);
const keyedA: any = new KeyedA(), keyedB: any = new KeyedB();
const keyedFnA: any = keyedA[keyA], keyedFnB: any = keyedB[keyB];
console.log('evaluation-symbols', keyedFnA === keyedA[keyA], keyedFnA !== keyedFnB, typeof keyedA[keyB], typeof keyedB[keyA]);
console.log('evaluation-symbol-names', keyedFnA.name, keyedFnB.name);
console.log('evaluation-symbol-prototypes', Object.getPrototypeOf(keyedFnA.call(keyedA)) === keyedFnA.prototype, Object.getPrototypeOf(keyedFnB.call(keyedB)) === keyedFnB.prototype);
const oldComputed: any = StaticOwner.computed;
function replaceComputed() { (StaticOwner as any).computed = function*() { yield 113; }; return 0; }
const orderedComputed: any = (StaticOwner.computed as any)(replaceComputed());
console.log('ordered-computed-prototype', Object.getPrototypeOf(orderedComputed) === oldComputed.prototype);
print('ordered-computed-next', orderedComputed.next());
const erasedStatic: any = StaticOwner;
const erasedComputed: any = erasedStatic.computed();
console.log('erased-computed-prototype', Object.getPrototypeOf(erasedComputed) === erasedStatic.computed.prototype);
print('erased-computed-next', erasedComputed.next());

// A source method name must not redirect a built-in receiver's call.
class NamedLog { *log() { yield 114; } }
console.log('generator-log', new NamedLog().log().next().value);

function* protocolTypeWitness() { yield 115; }
const protocolTyped = protocolTypeWitness();
gp.next = function(value: any, extra = 116) { return { done: 1, value: value + extra }; };
const protocolResult: any = protocolTyped.next(1);
console.log('protocol-result', protocolResult.value, protocolResult.done ? 'done' : 'more');
gp.next = function() { return 117; };
const primitiveResult: any = protocolTyped.next();
console.log('protocol-primitive', primitiveResult, primitiveResult.done);
gp.next = 0;
try { protocolTyped.next(); } catch (e) { console.log('protocol-noncallable', e instanceof TypeError); }
gp.next = original;
protocolTyped.return();

// A memo primed on the intrinsic holder must observe getters and argument order.
let protocolOrder = "";
const getterIterator: any = protocolTypeWitness();
getterIterator.next();
Object.defineProperty(gp, "next", {
  configurable: true,
  get() {
    protocolOrder += "get,";
    return function (value: any) { protocolOrder += "call,"; return { value, done: true }; };
  }
});
function protocolArgument() { protocolOrder += "arg,"; return 118; }
print("protocol-getter", getterIterator.next(protocolArgument()));
console.log("protocol-order", protocolOrder);
Object.defineProperty(gp, "next", { value: original, writable: true, configurable: true });
getterIterator.return();
const builtinIterator: any = protocolTypeWitness();
gp.next = Math.max;
console.log("protocol-builtin", builtinIterator.next(119, 120));
gp.next = original;
builtinIterator.return();

// One warm call site must preserve Get-before-arguments without admitting
// an object that merely inherits the protocol. Allocations in the argument
// also exercise the selected receiver across moving safepoints.
function* selectedProtocol() { yield 122; return 123; }
function selectedNext(receiver: any, argument: () => any) { return receiver.next(argument()); }
const selectedWarm: any = selectedProtocol();
print("selected-warm", selectedNext(selectedWarm, () => 0));
const selectedFake: any = Object.create(Object.getPrototypeOf(selectedWarm));
try { selectedNext(selectedFake, () => 0); } catch (e) { console.log("selected-brand", e instanceof TypeError); }
const selectedChanged: any = selectedProtocol();
let allocated: any = null;
print("selected-before-argument", selectedNext(selectedChanged, () => {
  Object.setPrototypeOf(selectedChanged, null);
  for (let i = 0; i < 128; i++) allocated = { value: i };
  return allocated.value;
}));
print("selected-detached-done", original.call(selectedChanged));

function* stateLocals() {
  const before = { value: 124 };
  yield before.value;
  const after = 125;
  return after + 1;
}
const localStates: any = stateLocals();
print("state-local-yield", localStates.next());
print("state-local-return", localStates.next());
function* abruptLocal() {
  const cleanup = { value: 127 };
  try { yield 1; } finally { console.log("state-local-finally", cleanup.value); }
}
const abruptStates: any = abruptLocal();
abruptStates.next(); print("state-local-abrupt", abruptStates.return(128));

// Repeated resumes use the ordinary entry-owned shape-chain proof. Shadowing
// or relinking its first holder must invalidate that same hot call site.
function* hotProtocol() { for (let i = 0; i < 40; i++) yield i; }
function hotNext(it: any) { return it.next(); }
const hotIterator: any = hotProtocol();
let hotSum = 0;
for (let i = 0; i < 32; i++) hotSum += hotNext(hotIterator).value;
console.log("hot-resume", hotSum);
const hotParent: any = hotProtocol.prototype;
hotParent.next = function() { return { value: 130, done: false }; };
print("hot-shadow", hotNext(hotIterator));
delete hotParent.next;
print("hot-restored", hotNext(hotIterator));
Object.setPrototypeOf(hotParent, { next() { return { value: 131, done: true }; } });
print("hot-relinked", hotNext(hotIterator));
Object.setPrototypeOf(hotParent, gp);
print("hot-link-restored", hotNext(hotIterator));

// The shared boundary owns executing and completion even while a yielding
// finally resumes. A failed inner resume must leave the outer one executing.
let active: any;
function checkReentry() {
  for (const op of ["next", "return", "throw"]) {
    try { active[op](132); } catch (e) { console.log("reentry", op, e instanceof TypeError); }
  }
}
function* guardedResume() {
  try { checkReentry(); yield 133; }
  finally { checkReentry(); yield 134; }
}
active = guardedResume();
print("guarded-next", active.next());
print("guarded-return", active.return(135));
print("guarded-complete", active.next());
function* escapingResume() { yield 136; throw 137; }
const escaping: any = escapingResume();
escaping.next();
try { escaping.next(); } catch (e) { console.log("escaping-error", e); }
print("escaping-done", escaping.next());
