// #9053: the esbuild `__export` descriptor literal `{ get, enumerable: true }`
// lowers to a direct accessor install (js_object_define_get_accessor) instead
// of allocating a two-field descriptor object and re-decoding it by name in
// js_object_define_property. pi's 13MB bundle runs ~1,245 of these at startup.
// This fixture pins the fast path's observable parity with node: an
// __export-style install loop, reads through the live getters, enumeration
// order, descriptor reflection, redefinition over a fast-installed getter
// (both the spec-forbidden and the spec-allowed shapes), and the near-miss
// literals ({get, enumerable: false} / 3-field) staying on the generic path.
const __defProp = Object.defineProperty;
const __export = (target: any, all: any) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

let hits = 0;
const state = { a: 1, b: 2, c: 3 };
const exportsObj: any = {};
__export(exportsObj, {
  a: () => { hits++; return state.a; },
  b: () => { hits++; return state.b; },
  c: () => { hits++; return state.c; },
});

// Reads go through the getters; re-export live-binding semantics.
console.log("reads:", exportsObj.a, exportsObj.b, exportsObj.c, "hits:", hits);
state.b = 20;
console.log("live-binding:", exportsObj.b, "hits:", hits);

// Enumeration order and full descriptor reflection must match node exactly.
console.log("keys:", JSON.stringify(Object.keys(exportsObj)));
const d: any = Object.getOwnPropertyDescriptor(exportsObj, "a");
console.log(
  "desc:",
  typeof d.get,
  "set=" + String(d.set),
  "enumerable=" + String(d.enumerable),
  "configurable=" + String(d.configurable),
  "writable=" + String(d.writable)
);
console.log("has:", "a" in exportsObj, Object.prototype.hasOwnProperty.call(exportsObj, "b"));

// The install is non-configurable (configurable omitted -> false), so
// redefining with a DIFFERENT getter must throw exactly like node...
let msg = "no-throw";
try {
  __defProp(exportsObj, "a", { get: () => 111, enumerable: true });
} catch (e: any) {
  msg = e.message;
}
console.log("redefine-different:", msg, "value:", exportsObj.a);

// ...while the no-change redefine (same getter object, same flags) is
// spec-ALLOWED on a non-configurable accessor: the fast path must not have
// frozen anything beyond what defineProperty semantics say.
const keep = Object.getOwnPropertyDescriptor(exportsObj, "c")!.get;
let msg2 = "no-throw";
try {
  __defProp(exportsObj, "c", { get: keep, enumerable: true });
} catch (e: any) {
  msg2 = e.message;
}
console.log("redefine-same:", msg2, "value:", exportsObj.c);

// A getter installed CONFIGURABLE via the 3-field literal (generic path) can
// then be overridden by the 2-field fast-path literal: the redefine retains
// configurable: true and swaps the getter.
const cfgTarget: any = {};
// (direct-callee form on purpose — the alias form is covered above)
Object.defineProperty(cfgTarget, "cfg", { get: () => "old", enumerable: true, configurable: true });
Object.defineProperty(cfgTarget, "cfg", { get: () => "new", enumerable: true });
const dc: any = Object.getOwnPropertyDescriptor(cfgTarget, "cfg");
console.log(
  "override:", cfgTarget.cfg,
  "enumerable=" + String(dc.enumerable),
  "configurable=" + String(dc.configurable)
);

// Near-miss literal `{ get, enumerable: false }` takes the generic path and
// still behaves: readable, non-enumerable, non-configurable.
const hidden: any = {};
__defProp(hidden, "h", { get: () => 42, enumerable: false });
const dh: any = Object.getOwnPropertyDescriptor(hidden, "h");
console.log(
  "hidden:", hidden.h,
  "keys=" + JSON.stringify(Object.keys(hidden)),
  "enumerable=" + String(dh.enumerable),
  "configurable=" + String(dh.configurable)
);

console.log("final-hits:", hits);
