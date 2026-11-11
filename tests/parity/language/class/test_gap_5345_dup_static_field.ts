// #5345 — a class may declare the SAME static field name more than once.
// Per ECMAScript (ClassDefinitionEvaluation), every static field initializer
// runs in declaration order against one shared slot; the last write wins. A
// later initializer can read the value the earlier one left via `this.f`.
//
// Pre-fix, Perry emitted one `@perry_static_<mod>__<Class>__<field>` LLVM
// global PER declaration, so two `static f = …` lines produced a redefined
// global and clang rejected the module ("redefinition of global
// '@perry_static_…__C__f'"). The fix dedups the global emission (one defining
// global per (class, field)) while still running every initializer in order.
// Mirrors test262 language/statements/class/elements/static-field-redeclaration.

class C {
  static f = "test";
  static f = (this.f as string) + "262";
  static g() {
    return 45;
  }
  static g = this.g();
}
console.log("(1) C.f:", C.f);
console.log("(2) C.g:", C.g);

// Three redeclarations, each reading the prior value — order must hold.
class Layered {
  static v = 1;
  static v = (this.v as number) + 10;
  static v = (this.v as number) * 2;
}
console.log("(3) Layered.v:", Layered.v);

// A redeclared field whose later initializer has a side effect: both run.
const order: string[] = [];
class Seq {
  static x = (order.push("first") as unknown) as number;
  static x = (order.push("second") as unknown) as number;
}
console.log("(4) Seq.x:", Seq.x, "order:", JSON.stringify(order));
