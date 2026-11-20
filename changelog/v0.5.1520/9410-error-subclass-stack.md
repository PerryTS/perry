### Fixed

- **An `Error` subclass now has a `.stack` and reports `[object Error]`.**
  `class A extends Error {}` produced instances whose `.stack` was `undefined`
  and whose `Object.prototype.toString` tag was `"[object Object]"`. The base
  class was fine — `new Error("x").stack` has always been a string — so only
  subclasses were affected, and the claude-code bundle has **93** of them and
  **106** `.stack` reads. `claude doctor` printed ~10 real frames and 14,573
  bytes of stderr under node; under perry it printed ` -     at <anonymous>`
  and 120 bytes. Silent: no error, just a missing trace.

  One root cause behind both symptoms. `class A extends Error {}` deliberately
  produces an ordinary `GC_TYPE_OBJECT` class instance rather than a
  `GC_TYPE_ERROR` `ErrorHeader`, so that the subclass's own fields have
  somewhere to live. `alloc_error` — the only place that fills
  `ErrorHeader.stack` — is therefore never reached, and neither is any
  `stack` on `Error.prototype`, which carries only `name` and `message`. The
  `[object Error]` branch of `js_object_to_string` is keyed on that same GC
  header byte, so a subclass fell through to the `class_id` block and out the
  `"[object Object]"` default.

  The class-id registry that answers this question already existed and was
  wired at four other sites — `instanceof Error`, `util.types.isNativeError`,
  `Error.prototype.toString`'s subclass arm, and prototype-chain resolution
  all consult `extends_builtin_error(class_id)`. Neither the tag nor the stack
  did.

  - `crates/perry-runtime/src/object/to_string_tag.rs` — tag a
    `extends_builtin_error` class instance `"Error"`, set *before* the
    `Symbol.toStringTag` hook so a subclass's own tag still wins (§20.1.3.6
    consults the tag property last).
  - `crates/perry-runtime/src/error_subclass_stack.rs` (new; `error.rs` was
    within 90 lines of the 2,000-line CI cap) — `js_error_subclass_capture_stack`
    installs the own, non-enumerable, configurable `stack` accessor node
    installs, capturing the FRAME at the construction site. The head
    (`"name: message"`) is formatted on read, not at capture, because that is
    what V8 does and what the ubiquitous
    `constructor(m) { super(m); this.name = "X" }` shape needs: node reports
    `"X: m"`, and the assignment happens after `super()` returns. A user
    `Error.prepareStackTrace` still wins, as it does for
    `Error.captureStackTrace`. The setter redefines `stack` as a plain data
    property, so `err.stack = ""` keeps working.
  - `crates/perry-runtime/src/object/class_constructors.rs` — install it from
    `js_error_subclass_default_init` (the synthesized standalone ctor, which
    also serves the dynamic-parent `super` path) and from
    `default_error_init_for_implicit_chain` (the dynamic `new` replay), the
    two runtime sites that already stamped `message`/`name` and stopped there.
    In the replay the install is moved above the message guard, which returns
    early for a no-argument `new X()` — exactly the instances that would
    otherwise still have no trace.
  - `crates/perry-codegen/src/expr/this_super_call.rs`,
    `crates/perry-codegen/src/lower_call/new_error_init.rs` (new; the
    static-`new` Error arm moved out of `new.rs`, which was 5 lines from the
    2,000-line CI gate) — the same call from the two codegen sites that stamp
    `message`/`name` inline: an explicit `super(message)` into a built-in
    Error, and the static-`new` arm for a subclass with no own constructor.
    `this` is reloaded from its slot first; the stamps above it can collect.

  A unit test in the new module installs the accessor under forced evacuation,
  which is the only condition that can expose an unrooted pointer — and which
  caught the first cut of that rooting reading a NaN-box handle back with
  `get_raw_const_ptr`, aborting every Error-subclass construction with
  "runtime handle kind mismatch". Nothing in the unit suite constructed an
  Error subclass before, so only a compiled probe saw it.

  Validation: `test-files/test_gap_9410_error_subclass_stack.ts`
  byte-compared against `node --experimental-strip-types` across a bare
  subclass, a `this.name`-assigning subclass, one with an extra field, a
  two-level subclass, a subclass that sets `message` after an argument-less
  `super()`, `TypeError`/`RangeError` subclasses, a factory-constructed
  instance, a caught throw, `Error.captureStackTrace` on a subclass, and
  controls for the base `Error`, a non-Error class and a plain object. The
  fixture asserts the portable parts of the contract — `typeof stack`, the
  head line, the `toString` tag, `name`/`message`/`instanceof`, and that
  `stack` is an own but non-enumerable property that stays out of
  `Object.keys` — because stack CONTENTS are host-specific. Demonstrated
  failing on a compiler built from unfixed `origin/main` (46 diverging lines).
