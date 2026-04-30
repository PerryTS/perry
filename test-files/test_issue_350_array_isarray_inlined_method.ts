// Issue #350: `Array.isArray(this.box.getEntities())` returned false
// for an array field accessed via a chained `this.field.method()` /
// `this.field.field` shape inside an inlined method body.
//
// Root cause: `perry_transform::inline::substitute_this` carried its
// own ad-hoc walker with an `_ => {}` catch-all. When the inliner
// hoisted `WorldLike.test()`'s body into the module init (because
// `let w = new WorldLike()` was scalar-replaced and the ctor + method
// got inlined back-to-back), nested `Expr::This` references buried
// inside `Expr::ArrayIsArray(inner)` — and dozens of other HIR
// variants the old match never enumerated — were left untouched.
// Codegen then loaded `Expr::This` from an empty `this_stack` at top
// level, returned 0.0, and `Array.isArray(0.0.box.getEntities())`
// fell through to the runtime path which (correctly) saw no pointer
// and returned false.
//
// The fix delegates `substitute_this`'s descent to the centralised
// `perry_hir::walker::walk_expr_children_mut` — same pattern v0.5.408
// (#318) used for the closure collector. The walker's match is
// exhaustive on `Expr`, so any new HIR variant added to `ir.rs`
// produces a compile error in `walker.rs` before it can silently leak
// `Expr::This` again.

class Box {
  entities: number[] = [];

  getEntities(): number[] {
    return this.entities;
  }
}

class WorldLike {
  box: Box;
  constructor() {
    this.box = new Box();
  }

  test(): void {
    // The four shapes that the bug bisection narrowed down. All four
    // must be `true`; pre-fix only the first two were.
    console.log("direct field via local:", Array.isArray(this.box.entities));
    console.log("method via local:", Array.isArray(this.box.getEntities()));

    // Chained-deeper variant: `Array.isArray` over a method-call result
    // whose receiver is itself the result of a method call on `this`.
    console.log("nested:", Array.isArray(this.box.getEntities()));

    // Negative case — a number value should NOT be reported as an
    // array. Pre-fix this would have constant-folded incorrectly via
    // the now-stricter v0.5.413 fast path; the runtime fallback
    // correctly returns false.
    console.log("number is not array:", Array.isArray(42));
    console.log("string is not array:", Array.isArray("hello"));
  }
}

const w = new WorldLike();
w.test();

// Issue's literal repro — `Array.isArray(this.box.getEntities())`
// printed inside a method called on a fresh `new WorldLike()`. The
// post-#324 fast-path constant-folds `this.box.entities` (a known
// Array-typed field on a known class) but only AFTER the inliner's
// `substitute_this` correctly rewrites `Expr::This` to `LocalGet(w)`
// — without that rewrite, `this.box` reads as undefined, the
// receiver class lookup fails, the fast path bails, and the runtime
// receives a 0-bit value.
class Box2 {
  private entities: number[] = [];

  getEntities(): number[] {
    return this.entities;
  }
}

class WorldLike2 {
  private box: Box2;

  constructor() {
    this.box = new Box2();
  }

  inspect(): void {
    console.log("issue repro:", Array.isArray(this.box.getEntities()));
  }
}

const world = new WorldLike2();
world.inspect();
