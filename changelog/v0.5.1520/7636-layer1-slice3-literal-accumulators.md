### Layer 1 rooting migration, slice 3 — the literal accumulators (#7615)

`expr/objects_arrays_lit.rs`, `expr/array_literal.rs`, `expr/object_literal.rs`
and `expr/array_push.rs` now root through `crate::rooting` and are listed in
`MIGRATED_MODULES`; none names `expr::temp_root`. Follows the template (#7617)
and slices 1a (#7618), 1b (#7620) and 2 (#7627).

**No new combinator, including where one looked necessary.** The spread
literal's growing array is `with_rooted_accumulator(Repr::Ptr)` + `advance`
(which fuses the republish to the call — the accumulator's address legitimately
changes on every append); the array literal's element group is
`with_operands_rooted`; the object literal's half-built handle is
`with_rooted_accumulator` on both build paths; `arr.push(...src)`'s operand pair
is `with_operands_rooted` over an empty window, which emits nothing.

The shape that looked like a fourth combinator is a `this`-capturing method
closure's deferred value: it is consumed by the `this` patch loop *below every
remaining initializer*, so its root must span exactly that suffix, and the
number of such values is data-dependent. None of the three
`with_operands_rooted*` forms can express it — all three lower their operand
list up front, which for an object literal would evaluate every property before
storing any of them and reorder observable side effects. `with_rooted_accumulator`
already *is* "a GC value held while more user code is lowered", so the answer is
one nested scope per such property, which falls out as a small recursion over
the property list. That replaces #6951's flat `Vec` of raw slots which were
never released at all — correct only because `temp_root_truncate` is a stack cut
and the object handle sat below them, i.e. correct because of an invariant
stated in a comment, and leaked outright on a `?` from a later initializer.

**Scoped honestly: three of the four modules are translations, not repairs.**
#7280 rooted the spread accumulator correctly, #6951 rooted the array literal's
element group and the object literal's handle correctly (including loading the
interned key *below* the value's lowering, which is the #7627 finding already
right in this arm). What the migration buys there is that the republish cannot
be forgotten and the release cannot become branch-conditional (#7462's shape,
which shipped in a sibling arm) — plus, in `array_literal.rs`, a release on the
two paths that had none: three exits each carried their own `temp_root_release`,
and the `?` on every element's lowering released nothing. `array_push.rs` named
no rooting symbol before the migration and names none after, so its ledger line
is vacuous on the committed source exactly as both slice-1 modules' were; only
the sabotage arm makes it an assertion, and the audit that earned the listing is
written into that file's header.

**A branch no corpus can reach, so it gets its own tests.**
`lower_object_literal`'s by-name path with a non-empty property list — the one
carrying the `this`-patch machinery — is not reachable from TypeScript: since
#809 every source-level literal containing a `Prop::Method` lowers to a
source-ordered IIFE over `{}`. Measured rather than assumed — over the whole
`gc_root_dominance_corpus.sh` corpus (129 sources, 149 modules) every emitted
`js_object_alloc` is `(i32 0, i32 0)`. Four unit tests built directly from HIR
now pin it: the by-name path is selected, both patches run below the last
property store, the patches are applied in **source order** (readable off the
reserved `this`-slot index, because the two closures deliberately reserve
different ones), and three slots are rooted and released innermost-first with
the object handle released last. Deleting the list's `reverse()` turns the third
red.

**Also found, filed rather than fixed: #7634.** `arr.push(f())` and
`arr.push(...g())` evaluate the receiver *after* the argument, so an argument
that reassigns the receiver's binding pushes onto the wrong array (node prints
`[9]`, perry prints `[9,2]`). That order is exactly what makes the arm
rooting-free today, so restoring spec order puts the receiver in a window and
every pointer-valued push gains a temp root — expressible with the existing
`with_operands_rooted_across`, but needing a measured before/after that a
behaviour-preserving refactor has no mandate to run.

**Verified locally** (the CI backlog is deep, so this is the evidence). IR A/B
against a baseline built from `c1365ed8a`'s copies of the five touched files:
the probes are byte-identical in both environments, the dependency-scale corpus
(81 zod modules, 64 MB) is byte-identical, and the curated corpus is 2450/2452
identical — the two differences being the same instruction pair in the
iterator-result literal, where the accumulator's re-read now lands *after* the
value's `bitcast` instead of before because it is fused to the emission that
consumes it. Identical opcode multiset, same operands, same call.
`gc-root-dominance` green in both gated modes on both corpora with an empty
allowlist (curated 2452 functions / 9846 root stores → 0 violations, 40/40
seeded, `--unrooted-allocas` 0 over 7867; dependency-scale 12899 functions /
12908 root stores → 0, 40/40, 0 over 15242), root-store counts unchanged on
both. `cargo test -p perry-codegen --lib` 699 pass and `--doc`'s two
`compile_fail,E0499` arms still reject; `-p perry-runtime --no-fail-fast` 1902
pass first run; `cargo check --all-targets` clean. 13 gap-family filters over
both prebuilt arms produce identical 140-verdict sets. All probes byte-identical
in stdout and exit code on both arms and again under `PERRY_GC_ZEAL=1
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800
PERRY_GC_MOVING_LOOP_POLLS=1`, with `PERRY_GC_DIAG=1` confirming the instrument
was live (5 retired from-space sets). Ledger sabotage run per module — each of
the four turns the ledger red naming both planted lines. It also caught a real
violation in this change's own new test, where an assertion message quoted
`temp_root_truncate`: that is a code line, not a comment.
