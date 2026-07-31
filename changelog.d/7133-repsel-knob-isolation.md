### Fixed

- **Representation-selection bisection knobs now move exactly one representation
  each (#7128).** Two of them did not, which is worse than an ordinary bug:
  every knob-based A/B taken through them measured two things and attributed the
  sum to one.

  - `PERRY_CANONICAL_I32_LOCALS=0` **also disabled every `Ptr<Shape>`
    consumption.** Phase 5a reused `repsel_context_allows_canonical_i32` as its
    context gate; #7121 split the `FnCtx` field but left the four ordinary-body
    construction sites (`codegen/function.rs`, `method.rs` ×2, `closure.rs`)
    initialising both fields from one `repsel_allows` bool whose first conjunct
    was the canonical-i32 env read. Census under that knob read `ptr-shape: 7
    selected, 0 consumed`, with all six consumption sites printing
    `NEVER FIRES`.
  - `PERRY_CANONICAL_STR_LOCALS=0` **was not scoped to `Str` locals.** Three
    Phase 3a lowerings key on a value's static string type and never on a
    selected local — the inline `StringRef` retag
    (`native_value/materialize.rs`), the proven-heap operand arm of
    `str_operand_handle_tag_dispatched` (`lower_string_method.rs`), and the
    tag-dispatched `.length` (`expr/property_get.rs`). They changed the emitted
    object on 23 of the 26 census workloads, 20 of which promote no
    `canonical-str` at all.

  New `expr::repsel_gates` holds the knob table and a pure
  `RepselGates -> RepselContextFlags` derivation that all six `FnCtx`
  construction sites go through, so "one knob moves one flag" is a unit-testable
  property instead of a convention. The three static-string lowerings move to
  their own `PERRY_STATIC_STRING_LOWERING` (keyed into the object cache, with a
  `gc_repsel_matrix.sh` arm keeping its off-state exercised).

  **The default build is byte-identical**: 26/26 census workloads, same census
  counts, on both compilers. The `Str` split is an exact partition — base with
  `PERRY_CANONICAL_STR_LOCALS=0` and this build with that knob *plus*
  `PERRY_STATIC_STRING_LOWERING=0` emit byte-identical objects on 26/26. The
  `Ptr<Shape>`, `Ptr<NumArray>` and int-valued-TA knob arms are 26/26 identical
  too.

### Added

- **`census-knob-isolation`** (`scripts/compiler_output_regression.py`) — a gate
  for the property every knob-based A/B silently assumes. Per knob, with that
  knob at `0`: no census key outside its own may change; a workload whose
  representation promotes nothing must emit a **byte-identical** object; and the
  knob must still be live (take a promotion away somewhere, change some object).
  Two controls guard the diff — the compiler must be deterministic, and both
  `X=1` and an env var the compiler does not read must reproduce the default
  object bit-for-bit.

  It fails on `main` (7 count leaks, 20 emission leaks) and passes here. One
  documented, downward-only exception: `PERRY_INT_VALUED_LOCALS=0` lowers
  `canonical-i32` by one on `fixture_int_valued_ta`, because
  `int_valued_ta_locals` is merged into `integer_locals` — a withdrawn proof
  cannot be selected. A knob that *raises* another representation's count is
  still a leak.

  Object emission is **nondeterministic on aarch64 Linux** (the LLVM temp module
  name embeds pid + nanotime and lands in the ELF object — filed as #7131), so
  the emission half detects the host and skips rather than reporting 26 phantom
  diffs. `--require-emission` turns that into a failure where determinism is
  expected.
