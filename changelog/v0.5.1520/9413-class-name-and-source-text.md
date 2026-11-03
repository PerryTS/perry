### Fixed

- **A class's compiler-internal identity no longer escapes into `.name`,
  `Function.prototype.toString`, or `util.inspect`.** Three separate leaks, all
  of the same shape: a registration key or a class id that only the compiler
  should ever see, handed to the program as a user-visible string.

  1. **`.name` reported the disambiguation key.** Two `class Made {}` in sibling
     function bodies are distinct classes, so the second registers under a
     uniquified key (`Made$0`) to keep the name-keyed dedup from aliasing the two
     bodies onto one ClassId — see `maybe_rename_colliding_class`. That key
     reached `js_register_class_name`, so `Made.name` and
     `new Made().constructor.name` answered `"Made$0"`.

  2. **A class expression constructed in place lost its name entirely.**
     `new (class extends Error {})("m").constructor.name` answered
     `"__anon_class_8"` (node: `""`), and even a *named* one —
     `new (class Q {})().constructor.name` — answered `"__anon_class_6"` instead
     of `"Q"`. `lower_new_non_ident` lowers straight to a `New` on a synthetic
     key and never recorded the spec name, while its sibling
     `lower_expr/arm_class.rs` had recorded exactly that override
     (`display_override`) since #5592.

     Both are fixed by populating the existing `Module::class_display_names`
     override that `codegen/string_pool.rs` already prefers over the
     registration key. No new mechanism.

  3. **`console.log(C)` and `util.inspect(C)` printed the raw class id.**
     `util.inspect(Klass)` answered `6`. A class ref shares the INT32 encoding
     with a tagged small integer, and the console formatter's `is_int32()` arm
     printed the payload. It now renders node's form — `[class Klass]`,
     `[class Sub extends Named]`, `[class (anonymous)]`.

- **`String(C)` / `C.toString()` now return the class's source text.** They
  returned `function Klass() { [native code] }`, which is not what node produces
  for a class and not something a caller can parse. Perry already retained
  function source (`Module::closure_source_text`, #4101) — the same
  span-slice-at-lowering mechanism, keyed by ClassId, was simply never applied to
  classes, which are the one callable kind that is not a `ClosureHeader` and so
  cannot recover source from the closure registry.

  `Module::class_source_text` is populated at lowering by slicing the module
  source against `ast::Class::span` (SWC anchors it at the `class` keyword and
  closes it at the body's `}`, so the slice is exactly the class's
  `[[SourceText]]`), emitted by codegen as `js_register_class_source`, and read
  by all three class-ref `toString` sites. A class with no registered source (a
  builtin, or one perry synthesized) still gets the `[native code]` form, which
  Test262's `assertToStringOrNativeFunction` accepts. Monomorphized
  specializations inherit the origin's source, for the same reason #7632 makes
  them inherit its name.

  Affected files:

  - `crates/perry-hir/src/lower_decl/class_decl.rs` — `capture_class_source`
    (the class sibling of `capture_function_source`), plus the display-name
    override for a renamed duplicate.
  - `crates/perry-hir/src/lower/expr_new/non_ident.rs` — record the spec `.name`
    of an in-place-constructed class expression.
  - `crates/perry-hir/src/ir/module.rs`,
    `crates/perry-hir/src/lower/{context,lowering_context,lower_module_fn}.rs`,
    `crates/perry-hir/src/stable_hash/module.rs`,
    `crates/perry-hir/src/monomorph/driver.rs` — the `class_source_text` map and
    its flush; it participates in the stable hash because it drives codegen.
  - `crates/perry-codegen/src/codegen/{string_pool,artifacts}.rs`,
    `crates/perry-codegen/src/runtime_decls/strings.rs` — emit
    `js_register_class_source`.
  - `crates/perry-runtime/src/object/class_registry/class_meta.rs` — the source
    side table, `class_ref_to_string`, `class_ref_inspect_label`.
  - `crates/perry-runtime/src/value/to_string.rs`,
    `crates/perry-runtime/src/object/native_call_method/common_methods.rs`,
    `crates/perry-runtime/src/object/global_this/array_error.rs`,
    `crates/perry-runtime/src/builtins/formatting.rs` — the four read sites.

  Not addressed, and still divergent: `String(C.prototype.m)` for a class
  METHOD returns `function () { [native code] }` (node returns the method's
  source). Class methods compile to `perry_method_*` symbols rather than
  closures with a registered source, so this needs the method-side equivalent of
  the closure source registry, not another read of this one. Object-literal
  methods already work and are kept in the fixture as the control.

  Validation: `test-files/test_class_name_and_source_9413.ts` (ESM) and
  `test-files/test_class_name_cjs_9413.cts` (CommonJS, for the
  `module.exports = class {}` spellings that get no NamedEvaluation), both
  byte-compared against `node --experimental-strip-types`.
