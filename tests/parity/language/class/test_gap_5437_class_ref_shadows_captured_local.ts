// Issue #5437 regression (Next.js app-page-turbo): a bare-ident reference to a
// captured outer-scope local must NOT resolve to a same-named `class`
// declared in a SIBLING function factory.
//
// Perry's class system is name-keyed + module-global, and minified bundles
// reuse short names (`ej`) for many distinct bindings. The lowerer kept a
// `forward_class_names` set (inherited into nested function bodies) so a
// `class X` declared in the current body could shadow a same-named OUTER
// captured local (p-timeout's `class a extends Error` overwriting an
// undefined placeholder `a`). That rule was too broad: the class name lingered
// in the inherited set while lowering a SIBLING factory, so a deeper nested
// closure's reference to its OWN captured local `ej` resolved to the
// `class ej` (= NextURL) reference instead of the local. The class ref then
// flowed into a WeakMap key (`O.set(ej, ...)`) and threw
// "Invalid value used as weak map key", 500ing every dynamic page route.
//
// The fix records the scope depth each forward class is declared at and, at a
// reference where both a same-named class and a local exist, applies JS
// nearest-binding rules: the class shadows the local only when it was declared
// at a deeper (nearer) scope.
//
// Expected output:
// NextURL
// 42
// inner-42
// 7

function makeClassFactory(): any {
  class ej {
    kind = "NextURL";
  }
  return ej;
}

function renderFactory(t: any): any {
  let ej = t; // captured local, referenced from the nested closure below
  if (t.flag) {
    ej = { ...t, value: t.value };
  }
  const inner = () => ej.value; // current-scope lookup misses; ej is captured
  const nested = () => {
    const deeper = () => ej; // even deeper closure: still the captured local
    return deeper().value;
  };
  return [inner(), nested()];
}

const C = makeClassFactory();
console.log(new C().kind);

const r = renderFactory({ value: 42, flag: false });
console.log(r[0]);
console.log("inner-" + r[1]);

// The class still wins when it genuinely shadows an outer placeholder (the
// p-timeout TimeoutError shape): an outer `Sentinel` is reassigned to a class
// declared in a nested body, and a closure in that body must read the class.
let Sentinel: any = undefined;
function timeoutFactory() {
  class Sentinel {
    code = 7;
  }
  const get = () => Sentinel; // class declared in THIS body shadows the outer
  return get();
}
const TE = timeoutFactory();
console.log(new TE().code);
