### Fixed

- **`path.*` read an SSO string's inline bytes as a `StringHeader` pointer (#7621).**
  `path.resolve("/root", computedShortString)` threw
  `TypeError [ERR_INVALID_ARG_TYPE]` where node returns the path, while the same
  call with a *literal* segment worked.

  Every `path.*` codegen arm unboxed its operand with `unbox_to_i64` — `bitcast
  double -> i64; and POINTER_MASK` — and handed the low 48 bits to a runtime
  entry that dereferences them as `*const StringHeader`. Those bits are the
  header for a **heap** string (`STRING_TAG` = 0x7FFF) and the **characters** for
  a small-string-optimized one (`SHORT_STRING_TAG` = 0x7FF9, length + up to
  `SHORT_STRING_MAX_LEN` = 5 inline bytes). A literal is interned onto the heap;
  a computed short string takes the inline form. The #214 class, bisected by
  length — 5 bytes threw, 6 bytes worked.

  Twelve arms were affected, not the five the issue named — `resolve(a, b)`,
  `resolve(p)`, `join`, `win32.join`, `normalize`, `extname`, `dirname`,
  `basename`, `basename(p, ext)`, `isAbsolute`, `parse` and `matchesGlob`, plus
  the `path.win32.*` equivalents. `parse` and `matchesGlob` were **silently
  wrong** rather than throwing (`path.parse(short).base` was `""`;
  `matchesGlob` matched the pattern against `""`), which is why they had never
  been reported. `path.relative`, `path.format` and `path.toNamespacedPath`
  already took NaN-boxed values (`js_path_relative_checked`, #2995) and were
  unaffected — that is the precedent this change generalises.

  Single-operand arms now call `js_path_arg_header`, which materialises **only**
  the SSO case and reproduces the old mask bit for bit for heap strings and for
  every non-string, so each entry point keeps its own established non-string
  behaviour (throw, or `unwrap_or_default`) unchanged. It is deliberately not
  `js_get_string_pointer_unified`, which coerces numbers to strings and would
  have turned `path.isAbsolute(5)` from Node's throw into `false`.

  Two-operand arms hand both operands to the runtime NaN-boxed
  (`js_path_*_value` in `crates/perry-runtime/src/path/value_args.rs`). Codegen
  cannot close that window itself: materialising the first operand allocates, and
  `rooting::with_operands_rooted` yields registers rather than slots, so the
  second operand's register is stale the instant the first is materialised and
  there is no re-read to reach for. The runtime entry roots the first operand in
  a `RuntimeHandleScope` and re-reads it through `RuntimeHandle::across_const`.

  That rooting half is defensive rather than instrument-proven, and the module
  says so at the site: reverting `across_const` to a pre-bound address produced
  zero faults under `PERRY_GC_ZEAL=1` + `PERRY_GC_PROTECT_FROMSPACE=1` (402k
  copying minors) and 0/400k mismatches under `PERRY_GC_FORCE_EVACUATE=1`,
  because a collection reached from inside an allocation runs with
  `GC_FLAG_IN_ALLOC` set and the copying minor is therefore ineligible — nothing
  moves at an allocation point today.

  Covered by `test-files/test_gap_7621_path_sso_operands.ts` (both sides of the
  SSO boundary, computed and literal operands, absolute and relative bases,
  multi-segment resolves, all twelve arms, the non-string throw) plus five
  `perry-runtime` unit tests.
