// Gap: `Object.defineProperty(SomeClass, key, descriptor)` (#7190).
//
// A static define on a CLASS was silently dropped — not misfiled, dropped:
// `C.zzz` was `undefined` and so was `new C().zzz`, so the value went nowhere.
// The cause is that `C` and `C.prototype` answer `class_ref_id` with the SAME
// class id (Perry maps the prototype ref back to its class), so the define path
// could not tell the two receivers apart and treated every one as a prototype
// install. That is right for `defineProperty(C.prototype, …)` — the drizzle
// `applyMixins` case that arm was written for — and wrong for `defineProperty(C, …)`.
//
// The user-visible form was zod: class errors reported
// `constructor.name === "Definition"` because the library renames constructors
// with `Object.defineProperty(Cls, "name", { value })`, and Perry kept
// resolving `.name` through the class registry.
//
// This test asserts all three receivers stay distinct — function, class,
// class prototype — and covers the attribute bits, because a static field and a
// `defineProperty` data descriptor share one side table but have opposite
// defaults: `static x = …` is writable+enumerable (CreateDataPropertyOrThrow),
// a data descriptor is neither. Getting that wrong does not show up in the
// value, only in `Object.keys` and the descriptor — which is exactly the shape
// that hid the original bug.

class D {}
Object.defineProperty(D, "name", { value: "Renamed" });
console.log("class-name:", D.name);
console.log("class-desc:", JSON.stringify(Object.getOwnPropertyDescriptor(D, "name")));

// A plain function was always correct; it must stay correct.
function f() {}
Object.defineProperty(f, "name", { value: "RenamedFn" });
console.log("fn-name:", f.name);

// Subclass, and the instance's view of it.
class Base {}
class Sub extends Base {}
Object.defineProperty(Sub, "name", { value: "SubRenamed" });
console.log("sub-name:", Sub.name);
console.log("ctor-name:", (new (Sub as any)() as any).constructor.name);

// Class expression.
const E = class {};
Object.defineProperty(E, "name", { value: "ExprRenamed" });
console.log("expr-name:", (E as any).name);

// An arbitrary key, not just `name` — the drop was general.
class G {}
Object.defineProperty(G, "zzz", { value: 7, configurable: true });
console.log("static-zzz:", (G as any).zzz);
// ...and it must NOT have landed on the prototype.
console.log("instance-zzz:", (new G() as any).zzz);

// Defining on the prototype still installs an instance member.
class H {}
Object.defineProperty(H.prototype, "pm", { value: 5, configurable: true });
console.log("proto-pm:", (new H() as any).pm);

// Enumerability: a data descriptor defaults to non-enumerable, a declared
// static field is enumerable, and both live in the same table.
class K {
  static declared = 1;
}
Object.defineProperty(K, "hidden", { value: 7 });
Object.defineProperty(K, "shown", { value: 8, enumerable: true });
console.log("keys:", JSON.stringify(Object.keys(K).sort()));
const seen: string[] = [];
for (const k in K) seen.push(k);
console.log("forin:", JSON.stringify(seen.sort()));
console.log("values:", (K as any).hidden, (K as any).shown, K.declared);
console.log("hidden-desc:", JSON.stringify(Object.getOwnPropertyDescriptor(K, "hidden")));
console.log("shown-desc:", JSON.stringify(Object.getOwnPropertyDescriptor(K, "shown")));
console.log("declared-desc:", JSON.stringify(Object.getOwnPropertyDescriptor(K, "declared")));
console.log("names:", JSON.stringify(Object.getOwnPropertyNames(K).sort()));
