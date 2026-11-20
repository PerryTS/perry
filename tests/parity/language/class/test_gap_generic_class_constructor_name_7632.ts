// #7632: `(new Gen<number>()).constructor.name` reported the MANGLED
// specialization name `Gen$num`. Perry monomorphizes `new Gen<number>()` into a
// second class (`monomorph/mangle.rs`), and the instance carries that class's
// id — but TypeScript erases type arguments, so node reports `Gen` for every
// specialization. The mangling is an implementation detail and must not reach
// user-visible names.
//
// The other half of the same leak was `instanceof`, fixed in #7575 / PR #7631
// by recording `Class::specialized_from`; this is the display-name half.

class Base {}
class Gen<T> extends Base {}
class GenNoExtends<T> {}

const a = new Gen<number>();
const b = new Gen(); // no type arguments: never specialized
const c = new GenNoExtends<number>();
console.log("instances:", a.constructor.name, b.constructor.name, c.constructor.name);

// The constructor binding itself was already correct — it names the generic.
console.log("ctor:", Gen.name, GenNoExtends.name);

// Two different specializations of one generic both report the generic's name.
class Pair<T> {
  v: T | undefined;
}
const p1 = new Pair<number>();
const p2 = new Pair<string>();
console.log("specializations:", p1.constructor.name, p2.constructor.name);

// Nested/chained generics.
class Wrap<T> extends Gen<T> {}
const w = new Wrap<number>();
console.log("nested:", w.constructor.name, w instanceof Gen, w instanceof Base);

// Surfaces worth pinning alongside the name (#7632's own checklist).
console.log("toString:", Object.prototype.toString.call(a));
class MyErr<T> extends Error {}
const e = new MyErr<number>();
e.message = "boom";
console.log("error:", e.name, String(e), e instanceof Error);

// The #7575 half must keep working.
console.log("instanceof:", a instanceof Gen, a instanceof Base, c instanceof GenNoExtends);
