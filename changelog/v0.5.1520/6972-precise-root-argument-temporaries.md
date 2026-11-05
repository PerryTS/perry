**fix(gc): argument temporaries are precise roots (#6951)**

With the conservative native-stack scan disabled — precise/shadow-stack roots
only — a collection landing during argument evaluation dropped `console.log`'s
string-literal argument, with no crash and no diagnostic. Harder shapes
(`fresh() + "/" + f()`, `new C(fresh(), f())`, `s.concat("|" + f())`) segfaulted.

**Root cause.** The shadow stack roots *named locals*: one slot per pointer-typed
local, bound to that local's alloca. It has no slot for the values that exist
only between two instructions, and an LLVM SSA register is not a GC root.
`console.log("alpha", churn())` lowers to `js_array_alloc(2)` plus one
`js_array_push_f64` per argument, with the accumulator threaded through an SSA
register. That register held the ONLY reference to everything already pushed —
argument 0 included — across argument 1's evaluation. The sweep freed the
half-built array, `churn` recycled the block, and the next push wrote the number
into a header whose `length` had been reset to 0: a one-element array, and the
label gone. Conservative stack scanning hid it, because `gc_check_trigger` forces
a full conservative scan on both automatic arms while `gc/roots.rs`'s nominal
production default is `Auto -> SkipDisabled` — the scan was doing load-bearing
correctness work, not acting as a safety net.

**Mechanism.** `crates/perry-runtime/src/gc/roots/temp_roots.rs` adds a
per-thread temp-root *stack* callable from generated code, registered in
`gc_init` as a budgeted mutable root scanner, so slots are marked AND rewritten
rather than pinned. Slots are visited through `visit_heap_word_u64_slot`, the
same decoder the shadow stack uses, so a slot may hold either word form the
`gc::root_words` contract admits: a NaN-boxed value or a bare heap address (the
raw `i64` array pointers threaded through `js_array_alloc`). Generated code
pushes before the collection point, **re-reads** after (mandatory — an
evacuating cycle rewrites the slot, so the pushed register is stale), and
truncates after the consuming call. Truncate is a stack cut, not a pop, so a
missed release is bounded by the next one. `ShadowSavepoint` now carries the
temp-root depth, so the `longjmp` unwind that already restores the shadow stack
restores this stack with it — no change to `crate::exception`.

**Rooted sites.** The variadic argument accumulator (`console.log` / `info` /
`warn` / `error` / `debug` / `trace` / `assert` / `timeLog`); the string-concat
operand pair and the n-way concat chain (template literals, log lines), plus the
intermediate `js_jsvalue_to_string` handle in the both-non-string fallback; the
object-literal handle across its initializers (all three lowering paths); and
array-literal element values.

**Cost.** Emission is gated three ways, any one of which suppresses it: nothing
after the value reaches a collection point; the value provably cannot be a heap
reference; or the value is a string literal (already a registered global root).
`"user_" + i`, `[1, 2, 3]`, `{a: i, b: total}` and all-local argument lists emit
byte-identical IR to before. On a hot loop doing a concat, an array literal, an
object literal and a template literal per iteration the gates take emitted
rooting calls from 32 to 12.

**Verification.** `scripts/gc_repsel_matrix.sh --arms all` against pinned Node
26.5.0: 361/361 cells byte-exact, FAIL=0, XFAIL=0 —
`test_gap_repsel_gc_stress × cons_scan_off` and `× cons_scan_off_force` move from
XFAIL to PASS with the arm measurably live (17 completed cycles), so both entries
are removed from `test-parity/gc_repsel_triage.txt` and `cons_scan_off` (a PR
arm) becomes a hard gate on this shape. A 431-file gap-corpus A/B against
`origin/main` produced identical result sets. Four new unit tests in
`gc::tests::temp_roots`, every one pinning `ConservativeStackScanMode::Disabled`
(with the scan on the bug is invisible), plus three codegen IR tests pinning the
emission contract and the no-cost gate.

**Still open**, filed with reproducers: #6968 (scalar-replaced object/array
locals), #6969 (`new C(a, b)` constructor arguments), #6970 (native-method-call
arguments), #6971 (string-method receiver + arguments). `moved_objects` remains 0
in every arm and 333 matrix cells remain UNVERIFIED — that is #6950, which this
change unblocks rather than fixes.
