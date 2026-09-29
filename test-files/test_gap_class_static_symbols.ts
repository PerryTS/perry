// Class static Symbol-keyed data properties are own symbol properties of the
// class function object: declared, runtime-added, defined, deleted, inherited.
const tag = Symbol("tag");
const extra = Symbol("extra");
const defined = Symbol("defined");
class C {
  static [tag] = "C-tag";
  static plain = 1;
}
class Sub extends C {}
console.log("declared", (C as any)[tag], tag in C, Object.prototype.hasOwnProperty.call(C, tag));
(C as any)[extra] = "runtime";
console.log("runtime", (C as any)[extra], extra in C);
console.log("symbols", Object.getOwnPropertySymbols(C).map(String).join(","));
console.log("inherited", (Sub as any)[tag], tag in Sub, Object.prototype.hasOwnProperty.call(Sub, tag));
console.log("sub symbols", Object.getOwnPropertySymbols(Sub).length);
Object.defineProperty(C, defined, { value: 42, enumerable: false });
console.log("defined", (C as any)[defined], Object.getOwnPropertyDescriptor(C, defined)!.enumerable);
console.log("ownKeys", Reflect.ownKeys(C).map(String).sort().join(","));
delete (C as any)[extra];
console.log("deleted", (C as any)[extra], extra in C, Object.getOwnPropertySymbols(C).map(String).join(","));
(Sub as any)[tag] = "Sub-tag";
console.log("shadow", (Sub as any)[tag], (C as any)[tag], Object.getOwnPropertySymbols(Sub).map(String).join(","));
class Even {
  static [Symbol.hasInstance](v: unknown) { return typeof v === "number" && v % 2 === 0; }
}
console.log("hasInstance", (4 as any) instanceof Even, (3 as any) instanceof Even);
class Branded { static [Symbol.for("brand")] = "b"; }
console.log("registered", (Branded as any)[Symbol.for("brand")], Symbol.for("brand") in Branded);
const is = (v: any, k: any) => Object.prototype.hasOwnProperty.call(k, tag) && v instanceof k;
console.log("is", is(new C(), C), is(new Sub(), Sub));
