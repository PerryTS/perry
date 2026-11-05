// DEC 250, locked built-ins: a member the realm image gives a built-in
// PROTOTYPE (except `constructor`) is never written by the program. Namespace
// members and constructor statics (Math.random, Date.now) stay patchable
// (owner ruling 2026-11-03).
// Replacing, deleting or redefining it, or relinking the object, is a
// TypeError at the store; adding a member works and keeps working. console,
// globalThis, fetch and process stay patchable. Node diverges by design, so
// the expected output is committed (test-parity/expected/).
const arr: number[] = [1, 2];
const P: any = Array.prototype;
function attempt(row: string, run: () => string): void {
  try {
    console.log(row, "no throw", run());
  } catch (e) {
    const err = e as Error;
    console.log(row, "throw", err instanceof TypeError ? "TypeError" : err.name, err.message);
  }
}

// (1) replace an image member
attempt("1", () => { P.push = function () { return 42; }; return String(arr.push(3)); });
console.log("1b push is the built-in:", arr.push(4), arr.length);
// (2) delete an image member
attempt("2", () => { delete P.push; return String(typeof P.push); });
// (3) redefine an image member
attempt("3", () => { Object.defineProperty(P, "push", { value: 1 }); return String(typeof P.push); });
// (4) add a member and call it
attempt("4", () => { P.myHelper = function (this: number[]) { return this.length * 10; }; return String((arr as any).myHelper()); });
// (5) defineProperty of a new member
attempt("5", () => { Object.defineProperty(P, "added", { value: 7, enumerable: false, writable: true, configurable: true }); return String(P.added); });
// (5b) the image stays locked after the program added members; the added member stays writable
attempt("5b", () => { delete P.push; return "deleted"; });
attempt("5c", () => { P.added = 8; P.myHelper = function () { return "second"; }; return String(P.added) + " " + (arr as any).myHelper(); });
// (6) constructor statics and namespaces
attempt("6a", () => { const o = Date.now; (Date as any).now = () => 0; const r = String(Date.now() === 0); (Date as any).now = o; return r; });
attempt("6b", () => { const o = Math.max; (Math as any).max = () => -1; const r = String(Math.max(1, 2)); (Math as any).max = o; return r; });
attempt("6d", () => { const o = Math.random; Math.random = () => 0.5; const r = String(Math.random()); Math.random = o; return r; });
attempt("6e", () => { const o = Date.prototype.constructor; Object.defineProperty(Date.prototype, "constructor", { value: o, writable: true, configurable: true }); (Date.prototype as any).constructor = o; return String(Date.prototype.constructor === Date); });
console.log("6c Date.now and Math.max are the built-ins:", Date.now() > 0, Math.max(1, 2));
// (7) console, globalThis.fetch, process stay patchable
attempt("7a", () => {
  const orig = console.log;
  (console as any).log = (...a: unknown[]) => orig("patched:", ...a);
  console.log("via patched log");
  (console as any).log = orig;
  return "restored";
});
attempt("7b", () => { (globalThis as any).fetch = () => "my fetch"; return String((globalThis as any).fetch()); });
attempt("7c", () => { process.exitCode = 1; const seen = process.exitCode; process.exitCode = 0; return String(seen); });
// (8) an add-only polyfill, applied twice
function polyfill(): string {
  if (!(P as any).at2) {
    P.at2 = function (this: number[], i: number) { return this[i < 0 ? this.length + i : i]; };
    return "installed";
  }
  return "present";
}
attempt("8a", () => polyfill() + " " + (arr as any).at2(-1));
attempt("8b", () => polyfill() + " " + (arr as any).at2(0));
// (9) relink a built-in prototype
attempt("9", () => { Object.setPrototypeOf(P, {}); return String(Object.getPrototypeOf(P) === Object.prototype); });
console.log("9b chain intact:", Object.getPrototypeOf(P) === Object.prototype, arr.hasOwnProperty("length"));
// (10) the interpreter runs the same funnels
const Run = Function;
attempt("10", () => { Run("Array.prototype.push = 1")(); return String(typeof P.push); });
// (11) other families and a symbol key
attempt("11a", () => { (Object.prototype as any).toString = () => "x"; return String({}.toString()); });
attempt("11b", () => { (Map.prototype as any).get = () => 1; return String(new Map().get(1)); });
attempt("11c", () => { delete P[Symbol.iterator]; return String(typeof P[Symbol.iterator]); });
attempt("11d", () => { (String.prototype as any).slice = () => ""; return "abc".slice(1); });
// (12) a program-added member can be re-set and deleted
attempt("12", () => { P.myHelper = 1; const had = P.myHelper; delete P.myHelper; return String(had) + " " + String(typeof P.myHelper); });
console.log("done", arr.length, [5, 6].map((x) => x + 1).join(","));
