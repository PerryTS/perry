// #9413: `.name` and `Function.prototype.toString` must report the SOURCE
// identity of a class, never the compiler's disambiguation key
// (`Made$0`, `__anon_class_8`).
import AnonDefault from "./_helpers/class_name_default_export_9413.ts";
import { inspect } from "node:util";

// --- Function.name: the whole node rule set -------------------------------
class Named {}
const AnonConst = class {};
let AnonLet = class {};
var AnonVar = class {};
const WithBinding = class Inner {};
class Sub extends Named {}

function nameOf(f: any) { return f.name; }

console.log("decl:", Named.name);
console.log("const:", AnonConst.name);
console.log("let:", AnonLet.name);
console.log("var:", AnonVar.name);
console.log("expr-binding:", WithBinding.name);
console.log("subclass:", Sub.name);
console.log("export-default:", AnonDefault.name);
console.log("arg-anon:", nameOf(class {}));
console.log("arg-named:", nameOf(class ArgNamed {}));
console.log("arg-anon-extends:", nameOf(class extends Named {}));

// Nested / shadowed same-name classes in sibling scopes: both are "Made".
function scopeA() { class Made {} return Made.name; }
function scopeB() { class Made {} return Made.name; }
class Made {}
console.log("shadowed:", Made.name, scopeA(), scopeB());

// Same, observed through an instance's constructor.
function ctorA() { class Dup {} return new Dup().constructor.name; }
function ctorB() { class Dup {} return new Dup().constructor.name; }
console.log("ctor-shadowed:", ctorA(), ctorB());

// A class expression constructed IN PLACE keeps the spec name.
console.log("new-anon:", new (class {})().constructor.name);
console.log("new-named:", new (class Zed {})().constructor.name);
console.log("new-anon-extends:", new (class extends Named {})().constructor.name);
console.log("new-anon-error:", new (class extends Error {})("m").constructor.name);
console.log("new-named-error:", new (class NErr extends Error {})("m").constructor.name);

// Nested function declarations keep their own name.
function outer() { function inner() {} return inner.name; }
console.log("nested-fn:", outer());

// `.name` is configurable: defineProperty replaces it.
class Renamed {}
Object.defineProperty(Renamed, "name", { value: "Custom" });
console.log("defineProperty:", Renamed.name);

// A `static name` member wins over the inferred name.
class StaticName { static name = "override"; }
console.log("static-name:", StaticName.name);

// --- Function.prototype.toString ------------------------------------------
class Klass { x = 1; m() { return this.x; } }
console.log("String:", String(Klass));
console.log("toString:", Klass.toString());
console.log("template:", `${Klass}`);
console.log("concat:", "" + Klass);
console.log("expr-toString:", String(class Anon { y = 2; }));
console.log("subclass-toString:", String(class ExtNamed extends Named {}));
console.log("objmethod-toString:", String(({ m() { return 1; } }).m));

// #9468: class member values retain their exact MethodDefinition source even
// though Perry compiles them as raw vtable/static/accessor symbols rather than
// ordinary closure bodies.
class MemberSource {
  m() { return 1; }
  static sm() { return 2; }
  get value() { return 3; }
  set value(v) { void v; }
  async am() { return 4; }
  *gm() { yield 5; }
  async *agm() { yield 6; }
}
const memberDescriptor = Object.getOwnPropertyDescriptor(MemberSource.prototype, "value")!;
console.log("method-String:", String(MemberSource.prototype.m));
console.log("method-toString:", MemberSource.prototype.m.toString());
console.log("method-template:", `${MemberSource.prototype.m}`);
console.log("static-method:", String(MemberSource.sm));
console.log("getter-source:", String(memberDescriptor.get));
console.log("setter-source:", String(memberDescriptor.set));
console.log("async-method:", String(MemberSource.prototype.am));
console.log("generator-method:", String(MemberSource.prototype.gm));
console.log("async-generator-method:", String(MemberSource.prototype.agm));

// Object-literal accessors use SetFunctionName with the `get`/`set` prefix.
const objectAccessors = {
  get g() { return 1; },
  set s(v) { void v; },
};
console.log(
  "object-accessor-names:",
  Object.getOwnPropertyDescriptor(objectAccessors, "g")!.get!.name,
  Object.getOwnPropertyDescriptor(objectAccessors, "s")!.set!.name,
);

// util.inspect / console.log of a class object.
console.log("direct:", Klass);
console.log("inspect:", inspect(Klass));
console.log("inspect-sub:", inspect(Sub));
console.log("inspect-anon:", inspect(AnonConst));

// Control: a plain small integer must still print as a number even when its
// value collides with a live class id. A class ref shares the INT32 NaN-box
// encoding with tagged small integers, so the `[class …]` rendering above is
// gated on the class-id registry — the same probe `String(C)` already used.
const six = 6;
console.log("int-control:", six, 1, 2, 3, 6, 7, [6, 7], { v: 6 }, String(6));
