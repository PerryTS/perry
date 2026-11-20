// #10796: a method call on a receiver typed as a `Union` containing a class
// (e.g. `Foo | undefined`) must dispatch to the class's own method when the
// method name collides with an `Array.prototype` name — not fold to the
// array fast path.
//
// Root cause: `crates/perry-hir/src/lower/expr_call/local_array_methods.rs`'s
// `is_user_class_instance` guard (the thing that stops a user class's own
// `find`/`map`/`filter`/… method from being rewritten to `Expr::ArrayFind`
// et al., since the runtime dispatch of the array fast path calls its
// argument as a *callback*) only matched `Type::Named`/`Type::Generic`
// directly. A receiver whose static type is `Type::Union([Type::Named(...),
// Type::Void])` fell through the match's `_ => false` arm, so the union type
// read as "not a user class instance", and `f.find(x)` below folded to
// `Expr::ArrayFind`, which called the string argument `x` as a per-element
// predicate — `TypeError: string "ul#fruits" is not a function`.
//
// This is exactly the shape cheerio's `load.ts` hits: `searchContext:
// Cheerio<AnyNode> | undefined`, whose `.find(selector)` is a CSS-selector
// method mixed onto `Cheerio.prototype` at runtime (`Object.assign(
// Cheerio.prototype, ..., Traversing, ...)`), not `Array.prototype.find`.
class Foo {
  find(x: string): string {
    return "custom-find:" + x;
  }
}

function make(flag: boolean): Foo | undefined {
  return flag ? new Foo() : undefined;
}

const f: Foo | undefined = make(true);
if (!f) {
  throw new Error("unreachable");
}
console.log(f.find("ul#fruits"));
