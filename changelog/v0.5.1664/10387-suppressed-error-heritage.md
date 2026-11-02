### Fixed

- `new SuppressedError(...) instanceof Error` is `true` again, and
  `Object.getPrototypeOf(SuppressedError.prototype) === Error.prototype` /
  `Object.getPrototypeOf(SuppressedError) === Error` now hold as ECMA-262
  requires.

  `SuppressedError` was missing from `is_native_error_subclass_constructor`, so
  the `globalThis` population loop never linked its prototype pair into the
  Error family — `SuppressedError.prototype`'s `[[Prototype]]` stayed
  `Object.prototype`. That was latent while `instanceof Error` answered from the
  class registry (`extends_builtin_error(CLASS_ID_SUPPRESSED_ERROR)`, which
  `js_suppressed_error_new` registers). It stopped being latent when
  `js_instanceof` began consulting the instance's RECORDED prototype chain FIRST
  for `class_id == CLASS_ID_ERROR`: the walk reached a chain with no
  `Error.prototype` in it, returned `Some(false)`, and short-circuited the
  registry fact that used to carry the answer. `test_gap_disposablestack_2875`
  went red on `main` between `87dc334920` and `f207cf6618` — the window
  containing that change — and stayed red.

  The fix is the missing link itself, not a special case in `instanceof`: the
  recorded chain is now correct, so the walk answers `true` on its own and
  `Error.prototype.toString` is inherited (`String(err)` is
  `"SuppressedError: both failed"`). `test_gap_disposablestack_2875` gained
  direct assertions for both prototype links, for the `TypeError` control, and
  for the inherited `toString`.
