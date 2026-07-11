// Issue #6233 follow-up (review): NON-class user bindings — `function Map()
// {}`, a const holding a constructor — also lexically shadow the built-in.
// Construction already routed through the dynamic path, but the var-decl
// type inference still typed the binding as the built-in (`Generic { base:
// "Map" }` / `Named("Uint8Array")` / `Named("URLSearchParams")`), so method
// calls on the constructed instance dispatched down the intrinsic fast
// paths instead of the instance's own members.

function Map(this: any) {
  this.tag = "fn-map";
  this.get = function (k: string): string {
    return "fn-get-" + k;
  };
  this.set = function (_k: string, _v: number): string {
    return "fn-set";
  };
}
const map = new (Map as any)();
console.log("FNMAP", map.tag, map.set("a", 1), map.get("a"));

function Uint8Array(this: any) {
  this.tag = "fn-u8";
  this.length = 99;
}
const u8 = new (Uint8Array as any)();
console.log("FNU8", u8.tag, u8.length);

function URLSearchParams(this: any) {
  this.tag = "fn-usp";
  this.size = -5;
}
const usp = new (URLSearchParams as any)();
console.log("FNUSP", usp.tag, usp.size);

const Set = function (this: any) {
  this.tag = "const-set";
  this.has = function (_v: number): string {
    return "own-has";
  };
} as any;
const set = new Set();
console.log("CONSTSET", set.tag, set.has(1));
