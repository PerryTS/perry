// A top-level function declaration stored into from anywhere in its scope is
// a mutable binding: every later call reads the stored value.
const k = "y";

// Reassigned inside a nested function declaration.
function pick(c: any): string { return c ? "a" : "b"; }
function rebind(): void { pick = function (c: any) { return k + String(c); }; }
rebind();
console.log("nested function:", pick(true));

// Reassigned inside a nested arrow.
function viaArrow(): string { return "old"; }
const swapArrow = () => { (viaArrow as any) = () => "arrow"; };
swapArrow();
console.log("nested arrow:", viaArrow());

// Reassigned inside a class method.
function viaMethod(): string { return "old"; }
class Swapper {
  swap(): void { (viaMethod as any) = () => "method"; }
}
new Swapper().swap();
console.log("class method:", viaMethod());

// Compound assignment.
function compound(): string { return "old"; }
function grow(): void { (compound as any) += "!"; }
grow();
console.log("compound:", typeof compound, String(compound).slice(-1));

// Destructuring targets: array, object shorthand, default, for-of head.
function destA(): string { return "old"; }
function destB(): string { return "old"; }
function destC(): string { return "old"; }
function destD(): string { return "old"; }
function destructure(): void {
  [destA] = [() => "array"] as any;
  ({ destB } = { destB: () => "object" } as any);
  ({ x: destC = () => "default" } = {} as any);
  for (destD of [() => "for-of"] as any);
}
destructure();
console.log("destructuring:", destA(), destB(), destC(), destD());

// Assignment before the declaration (hoisting).
(early as any) = () => "early";
console.log("before declaration:", early());
function early(): string { return "old"; }

// A shadowing local is a different binding: the declaration keeps its body.
function kept(): string { return "kept"; }
function shadow(kept: any): any { kept = () => "param"; return kept(); }
function shadowLet(): string { let kept = () => "let"; kept = () => "let2"; return kept(); }
console.log("shadowed:", shadow(null), shadowLet(), kept());

// Calls made through the binding before and after a later reassignment.
function later(): string { return "first"; }
const before = later();
function setLater(): void { (later as any) = () => "second"; }
setLater();
console.log("later:", before, later());
