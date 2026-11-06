// #7947: `WeakRef.prototype.deref` and `FinalizationRegistry.prototype.
// register`/`.unregister` existed ONLY as an HIR fold keyed on a bare local
// NAME recorded by `pre_scan_weakref_locals`. Unlike WeakMap/WeakSet they had
// no runtime method-dispatch fallback and no prototype thunks, so exactly two
// receiver shapes worked — a `const x = new WeakRef(...)` at module top level
// or inside a function DECLARATION. Every other shape threw
// `TypeError: deref is not a function`: an array element, an object property,
// a call result, a `for…of` binding, a function parameter, a `.map` callback,
// and any binding inside an arrow function / function expression / class method
// (the pre-scan descends into function declarations only).
//
// #7948: the fold is also scope-blind, so a module holding one genuine
// `new WeakRef(x)` under some name folded EVERY same-named receiver's
// `.deref()` onto `js_weakref_deref`, which read its internal slot by name off
// the foreign object and answered `undefined` — a wrong answer with exit code
// 0. The helpers now brand-check and fall back to ordinary dynamic dispatch.
//
// The `WeakMap`/`WeakSet` halves already worked through every receiver shape
// (they have both the dispatch arm and the prototype thunks) and are asserted
// here so a future refactor of the shared dispatch cannot silently drop them —
// but their #7948 collision half was equally broken, with far more common
// method names (`get`/`set`/`has`/`add`/`delete`), and is asserted too.
//
// Boundary this test pins DELIBERATELY as still-broken (so nobody reads a green
// run as coverage): weak-wrapper SUBCLASSING — `class M extends WeakMap {}` and
// the WeakRef/WeakSet/FinalizationRegistry equivalents — throws on `main` both
// before and after this change, and is out of scope here.

const target = { tag: "T" };
const key1 = { k: 1 };

// --- receiver shapes that used to throw --------------------------------
const arrA = [new WeakRef(target)];
console.log("array-element:", (arrA[0].deref() as any).tag);

const wrFromElement = arrA[0];
console.log("local-from-element:", (wrFromElement.deref() as any).tag);

console.log("new-inline:", (new WeakRef(target).deref() as any).tag);

const holder = { r: new WeakRef(target) };
console.log("object-property:", (holder.r.deref() as any).tag);

const wrFromProperty = holder.r;
console.log("local-from-property:", (wrFromProperty.deref() as any).tag);

function makeRef(): WeakRef<object> {
  return new WeakRef(target);
}
console.log("function-return:", (makeRef().deref() as any).tag);

const wrFromCall = makeRef();
console.log("local-from-call:", (wrFromCall.deref() as any).tag);

let forOfOut = "";
for (const wrLoop of arrA) {
  forOfOut = (wrLoop.deref() as any).tag;
}
console.log("for-of-binding:", forOfOut);

function viaParam(w: WeakRef<object>): string {
  return (w.deref() as any).tag;
}
console.log("function-param:", viaParam(new WeakRef(target)));

console.log("map-callback:", arrA.map((w) => (w.deref() as any).tag).join(""));

const mapOfRefs = new Map<string, WeakRef<object>>();
mapOfRefs.set("a", new WeakRef(target));
console.log("map-value:", (mapOfRefs.get("a")!.deref() as any).tag);

// The pre-scan visits function DECLARATIONS but not arrow bodies, function
// expressions, or class methods — those threw even for a directly-named local.
const arrowLocal = (): string => {
  const wrArrow = new WeakRef(target);
  return (wrArrow.deref() as any).tag;
};
console.log("arrow-fn-local:", arrowLocal());

const fnExprLocal = function (): string {
  const wrExpr = new WeakRef(target);
  return (wrExpr.deref() as any).tag;
};
console.log("fn-expr-local:", fnExprLocal());

class MethodHolder {
  go(): string {
    const wrMethod = new WeakRef(target);
    return (wrMethod.deref() as any).tag;
  }
}
console.log("class-method-local:", new MethodHolder().go());

// The two shapes that already worked — kept so the fix cannot regress them.
const wrTopLevel = new WeakRef(target);
console.log("direct-local:", (wrTopLevel.deref() as any).tag);
function declFnLocal(): string {
  const wrDecl = new WeakRef(target);
  return (wrDecl.deref() as any).tag;
}
console.log("decl-fn-local:", declFnLocal());

// --- the reflective / value-read path ----------------------------------
console.log("typeof-method:", typeof wrTopLevel.deref);
console.log("typeof-method-element:", typeof arrA[0].deref);
console.log("reflective-call:", ((WeakRef.prototype.deref as any).call(wrTopLevel) as any).tag);
const boundDeref = wrTopLevel.deref.bind(wrTopLevel);
console.log("method-extract:", (boundDeref() as any).tag);
console.log("proto-identity:", wrTopLevel.deref === WeakRef.prototype.deref);
console.log("optional-call:", (wrTopLevel.deref?.() as any).tag);
console.log("deref-length:", WeakRef.prototype.deref.length);
console.log("toString-tag:", Object.prototype.toString.call(wrTopLevel));
try {
  (WeakRef.prototype.deref as any).call({});
  console.log("brand-check:", "NO THROW");
} catch (e) {
  console.log("brand-check:", (e as Error).name);
}

// --- FinalizationRegistry through the same shapes ----------------------
const regArr = [new FinalizationRegistry<number>(() => {})];
regArr[0].register(target, 1, target);
console.log("finreg-array-element:", regArr[0].unregister(target));

const regHolder = { f: new FinalizationRegistry<number>(() => {}) };
regHolder.f.register(target, 2, target);
console.log("finreg-object-property:", regHolder.f.unregister(target));

const finregArrow = (): boolean => {
  const regArrow = new FinalizationRegistry<number>(() => {});
  regArrow.register(target, 3, target);
  return regArrow.unregister(target);
};
console.log("finreg-arrow-fn-local:", finregArrow());
console.log("finreg-register-length:", (FinalizationRegistry.prototype.register as any).length);
console.log(
  "finreg-toString-tag:",
  Object.prototype.toString.call(new FinalizationRegistry<number>(() => {})),
);

// --- #7948: a same-named foreign receiver must keep its OWN method ------
class Cache {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  deref(): number {
    return this.v * 10;
  }
  register(a: number): string {
    return "user-register:" + a;
  }
  unregister(a: number): string {
    return "user-unregister:" + a;
  }
}

// `wrTopLevel` above is the genuine WeakRef binding that makes the name-keyed
// pre-scan fold every same-named `.deref()` in this module. These four bind the
// SAME identifier to something else; each must resolve its own method.
function collideObjectLiteral(): unknown {
  const wrTopLevel = { deref: () => 40 };
  return wrTopLevel.deref();
}
function collideUserClass(): unknown {
  const wrTopLevel = new Cache(4);
  return wrTopLevel.deref();
}
function collideArrayProp(): unknown {
  const wrTopLevel: any = [1, 2, 3];
  wrTopLevel.deref = () => 41;
  return wrTopLevel.deref();
}
function collideParam(wrTopLevel: { deref: () => number }): unknown {
  return wrTopLevel.deref();
}
console.log("collide-object-literal:", collideObjectLiteral());
console.log("collide-user-class:", collideUserClass());
console.log("collide-array-prop:", collideArrayProp());
console.log("collide-param:", collideParam({ deref: () => 42 }));

// Same for the FinalizationRegistry names (`regArrow` is the genuine binding).
function collideRegister(): unknown {
  const regArrow = new Cache(1);
  return regArrow.register(3);
}
function collideUnregister(): unknown {
  const regArrow = new Cache(1);
  return regArrow.unregister(4);
}
console.log("collide-register:", collideRegister());
console.log("collide-unregister:", collideUnregister());

// --- boundary: WeakMap / WeakSet already worked; keep them working -----
const wmArr = [new WeakMap<object, number>()];
wmArr[0].set(key1, 7);
console.log("weakmap-array-element:", wmArr[0].get(key1), wmArr[0].has(key1));
console.log("weakmap-delete:", wmArr[0].delete(key1), wmArr[0].has(key1));

const wsArr = [new WeakSet<object>()];
wsArr[0].add(key1);
console.log("weakset-array-element:", wsArr[0].has(key1));

const wmReflective = new WeakMap<object, number>();
wmReflective.set(key1, 3);
console.log("weakmap-reflective:", (WeakMap.prototype.get as any).call(wmReflective, key1));

// #7948 for the weak COLLECTIONS: `wmReflective`/`wsArr` above are the genuine
// bindings that make the name-keyed fold claim these identifiers module-wide.
// The poison pass only recognises `new OtherClass()` and call/await initializers,
// so an object literal, an array, or a parameter slipped straight through to
// `js_weakmap_get` and answered `undefined`.
function collideLiteralGet(): unknown {
  const wmReflective = { get: (k: string) => "lit:" + k };
  return wmReflective.get("a");
}
function collideParamGet(wmReflective: { get: (k: string) => string }): unknown {
  return wmReflective.get("b");
}
function collideArrayGet(): unknown {
  const wmReflective: any = [];
  wmReflective.get = (k: string) => "arr:" + k;
  return wmReflective.get("c");
}
function collideLiteralSetHasDelete(): string {
  const wmReflective = {
    set: (k: string, v: number) => "set:" + k + v,
    has: (k: string) => "has:" + k,
    delete: (k: string) => "del:" + k,
  };
  return (
    wmReflective.set("x", 1) + "/" + wmReflective.has("y") + "/" + wmReflective.delete("z")
  );
}
function collideLiteralAdd(): unknown {
  const wsArr = { add: (v: number) => "add:" + v };
  return wsArr.add(9);
}
console.log("collide-literal-get:", collideLiteralGet());
console.log("collide-param-get:", collideParamGet({ get: (k: string) => "p:" + k }));
console.log("collide-array-get:", collideArrayGet());
console.log("collide-literal-set-has-delete:", collideLiteralSetHasDelete());
console.log("collide-literal-add:", collideLiteralAdd());
console.log("weakmap-toString-tag:", Object.prototype.toString.call(wmReflective));
console.log("weakset-toString-tag:", Object.prototype.toString.call(new WeakSet<object>()));

// The deref result is still the live object, not a copy.
console.log("identity:", arrA[0].deref() === target);
