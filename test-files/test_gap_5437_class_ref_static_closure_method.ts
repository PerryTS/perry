// Issue #5437: calling a class static DATA property that holds a captured
// closure as a METHOD (`C.viaFn()`) where the class value reached the call as
// a class-reference (INT32-tagged class id) rather than a heap class object —
// i.e. the static analyzer could not prove the receiver is a class. Such
// statics live in the class dynamic-property side table, not the static-method
// vtable, and a class reference is not a heap object, so the dynamic method
// dispatcher resolved nothing and the call returned `undefined`/null. The read-
// then-call form (`const f = C.viaFn; f()`) already worked; the fused method
// call must match it.
//
// Expected output:
// returned: V
// aliased: V
// captured-in-ctor: scc:MF
// object-holder: V

// A class returned from a function, with a captured-closure static assigned
// after the class declaration. The method call must resolve the static.
function makeReturned() {
  const uw = "V";
  function getCache() {
    return uw;
  }
  class C {}
  (C as any).viaFn = getCache;
  return C;
}
console.log("returned:", (makeReturned() as any).viaFn());

// The class value flows through a local alias before the method call, so the
// receiver is a bare class reference at the call site.
function makeAliased() {
  function getCache() {
    return "V";
  }
  class C {}
  (C as any).viaFn = getCache;
  return C;
}
const Aliased = makeAliased();
const D = Aliased;
console.log("aliased:", (D as any).viaFn());

// The #5437 shape: a deferred-binding value captured by a class whose method
// reads it through the captured reference, dispatched on a returned class.
function makeIncremental() {
  const mod = {
    SharedCacheControls: class {
      manifest: string;
      constructor(m: string) {
        this.manifest = m;
      }
      hello() {
        return "scc:" + this.manifest;
      }
    },
  };
  function build(m: string) {
    return new mod.SharedCacheControls(m);
  }
  class IncrementalCache {}
  (IncrementalCache as any).build = build;
  return IncrementalCache;
}
console.log("captured-in-ctor:", (makeIncremental() as any).build("MF").hello());

// Control: the same shape on a plain object holder already worked; keep it
// green so a regression here is visible too.
function makeObject() {
  function getCache() {
    return "V";
  }
  const o: any = {};
  o.viaFn = getCache;
  return o;
}
console.log("object-holder:", makeObject().viaFn());
