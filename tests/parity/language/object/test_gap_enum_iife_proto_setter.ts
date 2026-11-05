// An enum IIFE stores through ordinary [[Set]]: an accessor that a program
// installed on Object.prototype for a member name (or an index) runs, and the
// member then never becomes an own property. An image birth that defines
// own properties must not apply here.

// DEC 250 (intrinsic-only): the IIFE argument is `E || {}`, whose prototype is
// always Object.prototype, so the inherited setter can only live there. The
// adds are refused now (expected file); the enum and image births still run.
let log = "";
try { Object.defineProperty(Object.prototype, "Trap", {
  set(v: any) { log += "set Trap " + v + "; "; },
  get() { return "inherited"; },
  configurable: true,
}); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
try { Object.defineProperty(Object.prototype, "1000", {
  set(v: any) { log += "set 1000 " + v + "; "; },
  configurable: true,
}); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
var E = /* @__PURE__ */ ((E2: any) => {
  E2[E2["A"] = 0] = "A";
  E2[E2["Trap"] = 1] = "Trap";
  E2[E2["K"] = 1000] = "K";
  E2[E2["D"] = 3] = "D";
  return E2;
})(E || {});
var P = /* @__PURE__ */ ((P2: any) => {
  P2["__proto__"] = "x";
  P2[P2["B"] = 0] = "B";
  return P2;
})(P || {});
const trap = E.Trap;
delete (Object.prototype as any).Trap;
delete (Object.prototype as any)["1000"];
console.log(JSON.stringify(E), Object.keys(E).join(","), trap, E.Trap, E[1000]);
console.log(log);
console.log(JSON.stringify(P), Object.getPrototypeOf(P) === Object.prototype);
