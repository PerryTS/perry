### Fixed

- Loops no longer consume stack on every iteration when they call a
  `Date.prototype.set*` setter, `Date.UTC`, `arr.concat`, `arr.splice`,
  `arr.toSpliced`, `arr.unshift` or `Array.prototype.{push,unshift,splice,concat}.call`
  (#10463). Such a loop died with SIGSEGV after about 2^19 iterations at the
  default 8 MB stack (`d.setTime(i)` used 16 B per iteration, and the crash point
  moved with `ulimit -s`). date-fns `addMinutes` in a loop crashed the same way,
  because the cross-module inliner copies its `setTime` into the caller's loop.

  These lowerings emitted their argument buffer (`alloca [N x double]`) or
  out-parameter (`alloca i64`) into whatever block was current. LLVM lowers an
  `alloca` outside the entry block to a runtime stack-pointer bump that is only
  released when the function returns. #167 added
  `LlFunction::alloca_entry_array` for one family of call sites; these sibling
  sites were never converted: `lower_date_setter` and `ArrayToSpliced`
  (`expr/os_uri_dates.rs`), `Date.UTC` (`expr/misc_methods.rs`), the
  `concat`/`unshift`/`splice` arms of `lower_array_method.rs`,
  `Expr::ArraySplice` (`expr/instance_misc1.rs`) and the array-like `.call`
  arms (`expr/logical_collections.rs`). Other sites had the same pattern: the
  multi-target dynamic `import()`/`require` and i18n join slots
  (`expr/dyn_extern_i18n.rs`), `new Worker` (`expr/worker_new.rs`), the V8
  interop argument buffers (`expr/v8_interop.rs`), the fused `push` length slot
  (`lower_call/native/native_instance_branch.rs`) and the module namespace
  populator (`codegen/helpers.rs`). All of them now allocate through
  `alloca_entry` / `alloca_entry_array` / `lower_js_args_array`. Each buffer is
  still filled completely right before its call.

  So the class cannot come back one call site at a time, the invariant is now
  enforced where every function body is finalized:
  `LlFunction::for_each_final_item` (read by both the textual and the native
  backend) refuses any `alloca` outside the entry block, whether typed or raw
  text, inside a multi-line raw payload, or after an inline invoke-EH label in
  block 0. The panic message names the function, block and instruction
  (`function/entry_allocas.rs`).

  Validation: the gap test `test_gap_10463_entry_block_allocas` crashes on the
  baseline (every section crashes on its own at 8 MB) and matches Node with the
  fix. `expr::entry_block_alloca_tests` compiles each construct inside a counted
  loop and reads the IR back with its own scanner. `function::entry_allocas::tests`
  sabotage-test the refusal. A `--no-link --trace llvm` sweep over all 1659
  `test-files/*.ts` found non-entry allocas in 61 files before the fix and 0
  after. Instruction counts are neutral to slightly lower (−0.08% to −0.36% on
  date-setter, `concat`, `Array.prototype.*.call` and `splice` loops).
