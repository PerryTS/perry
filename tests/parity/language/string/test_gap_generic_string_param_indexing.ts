// #321 frontier: indexing into a generic-typed `<T extends string>` parameter
// must produce a properly NaN-boxed string at the function-call boundary.
//
// Pre-fix, perry's codegen took the polymorphic-object IndexGet runtime
// helper for `self[0]` because the param's static type was `Type::Named("T")`
// (arrow type params weren't enter-scoped) or `Type::TypeVar("T")` (fn-decl
// scope entered but no constraint substitution). That helper read a
// `StringHeader*` as `ArrayHeader*` and returned the header's leading bytes
// as a subnormal `f64`, surfacing in effect's `Str.capitalize` /
// `Capitalize<T>` utilities as `1.5E-323oo` instead of `Foo`.
//
// Fix: in HIR lowering, when a type-parameter reference resolves to a
// runtime-narrowing upper bound (`<T extends string>`, `<T extends number>`,
// `<T extends boolean>`, `<T extends bigint>`, `<T extends U[]>`) substitute
// the constraint type at the `TsTypeRef` site so the param's static type is
// `String`/`Number`/etc. everywhere downstream, and the codegen IndexGet
// fast path fires.

// Arrow-form (the original repro shape).
const innerG = <T extends string>(s: T): string => s.toUpperCase();
const innerN = (s: string): string => s.toUpperCase();
const outerGtoInnerG = <T extends string>(self: T): string => innerG(self[0]) + self.slice(1);
const outerGtoInnerN = <T extends string>(self: T): string => innerN(self[0]) + self.slice(1);
const outerNtoInnerG = (self: string): string => innerG(self[0]) + self.slice(1);
console.log("ARROW G->G:", outerGtoInnerG("foo"));
console.log("ARROW G->N:", outerGtoInnerN("foo"));
console.log("ARROW N->G:", outerNtoInnerG("foo"));

// Effect-shape Capitalize<T>: charAt(0).toUpperCase() + slice(1).
const cap = <T extends string>(s: T): string => s.charAt(0).toUpperCase() + s.slice(1);
console.log("CAP:", cap("foo"));
console.log("CAP:", cap("a"));
console.log("CAP:", cap(""));

// Nested generic arrows — inner T captures outer's substituted type.
const outer = <T extends string>(s: T): string => {
  const inner = <U extends string>(u: U): string => u[0] + s;
  return inner("Q");
};
console.log("NESTED:", outer("abc"));

// Indirect call through a function-typed local — bypasses generic-call
// monomorphization, so the function body needs the constraint baked in
// at lowering time. Pre-fix this surfaced the same `1.5E-323oo` pattern.
function genFn<T extends string>(self: T): string { return self[0].toUpperCase() + self.slice(1); }
const fn: (s: string) => string = genFn;
console.log("FNPTR:", fn("foo"));

// Generic typed-array param.
const firstStr = <T extends string[]>(a: T): string => a[0];
console.log("ARR:", firstStr(["alpha", "beta"]));

// `<T extends number>` keeps numeric-typed fast paths.
const numAdd = <T extends number>(x: T): number => x + 1;
console.log("NUM:", numAdd(41));

// Unconstrained generics (`T extends unknown` / no constraint) are NOT
// substituted — they keep their `TypeVar(T)` shape so existing
// native-instance tagging / class-id propagation still apply.
const idAny = <T>(x: T): T => x;
console.log("ID:", idAny(42));
console.log("ID:", idAny("z"));
