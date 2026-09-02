### Fixed

- **Same-name `class` declarations at different lexical depths are distinct
  classes again — the inner body is no longer silently dropped.** Perry keeps
  two same-named classes apart by registering the second under a uniquified key
  (`M$0`); the key was minted once per **name** instead of once per **scope**, so
  the third and every later `class M` aliased onto the second's ClassId. Whichever
  body registered first won, and the program ran the wrong methods with no
  diagnostic:

  ```ts
  class M { v() { return "top"; } }
  function h() {
    class M { v() { return "outer"; } }
    function h2() { class M { v() { return "inner"; } } return new M().v(); }
    return [new M().v(), h2()].join(",");
  }
  console.log(new M().v(), h());   // node: top outer,inner   perry: top outer,outer
  ```

  Two defects, one symptom:

  1. **The "already renamed" guard was per name, not per scope.**
     `maybe_rename_colliding_class` returned early on
     `class_renames.contains_key(name)` — but `class_renames` is inherited by
     nested bodies (it is saved and restored per body, so an enclosing body's
     alias is live while the nested one lowers). A nested body declaring the same
     name therefore took the early return and registered its `class X` under the
     **outer** body's key. The map now carries the source span of the scope that
     minted each alias, so the guard means "this scope already renamed it" — the
     idempotence the guard existed for — and every nested scope mints its own.

  2. **Block scopes never ran the disambiguation scan at all.** Only function
     bodies did, so two sibling `{ class Blk { … } }` blocks shared one ClassId
     and the second ran the first's body:

     ```ts
     { class Blk { v() { return "b1"; } } console.log(new Blk().v()); }  // b1
     { class Blk { v() { return "b2"; } } console.log(new Blk().v()); }  // node b2, perry b1
     ```

     `class` is block-scoped, so the scan is now bracketed at every `{ … }`-shaped
     scope — bare block, `if` / `else` branch, loop body, `try` / `catch` /
     `finally`, and `switch` — mirroring `register_block_forward_lexicals`
     (#6062), which brackets the same boundary for TDZ names: record only what the
     scope changed, undo exactly that, so an alias owned by an enclosing scope
     survives.

  This is an **identity** fix, not a naming one: each declaration now gets its own
  ClassId, so `instanceof` across the shadowing boundary is correct in both
  directions (an inner instance is not `instanceof` the outer class, and vice
  versa), `Object.getPrototypeOf(inner) !== Outer.prototype`, and `class Sub
  extends M` inside the inner scope extends the **inner** `M`. `.name` keeps
  reporting the source name for all of them — the display-name override #9413
  (PR #9465) installed on this exact path is what carries it, and every newly
  minted alias goes through the same `lower_class_decl` site that records it.

  Validated by `test-files/test_gap_9466_shadowed_class_identity.ts`, byte-identical
  to `node --experimental-strip-types`.
