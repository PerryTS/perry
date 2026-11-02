### Fixed

- **`X.prototype.m = v` on a block-scoped class now patches that class, not an
  earlier class with the same name.** A `class X` that shadows or follows a
  same-named class registers under a scope-local key (#9466), but the
  prototype-method recognizer looked the class up by its raw source name, so
  the write was registered on the other `X`. Instances of the class the program
  actually patched never saw it, and the other class's instances did:

  ```ts
  { class B {} class C extends B {} void new C(); }
  { class B {} class C extends B {} (B.prototype as any).j = 6;
    console.log((new C() as any).j); }   // node: 6   perry: undefined
  ```

  The recognizer now resolves the binding through the active scope-local key
  before every class-keyed check (the class lookup, the accessor guard and the
  registration). This covers sibling blocks, nested blocks shadowing an outer
  class, loop bodies, and blocks inside a function body.

  Validated by `crates/perry/tests/block_class_identity.rs` against the
  `tests/fixtures/block_class_identity` fixture (node's output).
