The representation-selection census (#7104, #7113) counted `select()` calls.
That is the wrong quantity: a promotion can be selected, reported as a win, and
produce **literally nothing**. It now counts promotions codegen actually
*consumed*, and names the mechanism behind every one it did not.

`batch.ts` reports two `Ptr<Shape>` promotions and applies one. `totals` is
proven, counted as a win, and keeps the guarded diamond at every access site;
the entire 1,532-byte binary saving in #7107 came from `acc` alone. That was
found by reading emitted IR, never by the report.

Worse, the three census workloads that promote (`07_object_create`,
`09_method_calls`, `12_binary_trees`) were proposed as the cleanest available
experiment for measuring what a promotion is worth. **All three would have
measured 0.00%** — each declares its promoted local at module top level, and
`07`/`12` are additionally scalar-replaced. Compiling them with
`PERRY_PTR_SHAPE_LOCALS=0` and with the default produces a byte-identical
object. `09` differs only by two dead `__pshape` clones with zero call sites.

## The numbers

Across the 23-workload corpus, `Ptr<Shape>`: **6 selected, 2 consumed.** Four
proven and thrown away — two by the module-init context gate (#7109), two by
scalar replacement (#7115). No unexplained residue.

| workload | selected | consumed | mechanism |
|---|---|---|---|
| `fixture_ptr_shape` | 1 | 1 | — |
| `batch` | 2 | 1 | `module_init_context` |
| `suite_07_object_create` | 1 | 0 | `scalar_replaced` |
| `suite_09_method_calls` | 1 | 0 | `module_init_context` |
| `suite_12_binary_trees` | 1 | 0 | `scalar_replaced` |

## Three ways a promotion goes unconsumed

1. **The module-init / program-entry context gate** (#7109). `codegen/entry.rs`
   sets `repsel_context_allows_canonical_i32: false`, and
   `FnCtx::ptr_shape_receiver_fact` returns `None` for the whole body when that
   flag is clear — so an env knob for a *different* representation phase
   silently disables `Ptr<Shape>` consumption too.
2. **The same gate in async / generator bodies** (#6328).
3. **Scalar replacement deleted the object** (`collectors/escape_news.rs`) —
   previously undocumented, now **#7115**. Not a defect: deleting the
   allocation beats promoting it, and the passes are complementary (one in-loop
   field store flips a workload from scalar-replaced to `Ptr<Shape>`-consumed).
   The defect was that "scalar-replaced" and "promoted but wasted" rendered
   identically and mean opposite things.

## What changed

- `Outcome` gains `Consumed` / `Unconsumed`. Consumption is recorded at the six
  codegen sites that *commit* to the guard-free lowering — never at a
  `select()`-adjacent site, which would rebuild the same illusion one layer
  down. `outcome` (and, for consumed entries, the consuming site) joins
  `dedup_key`; without it every consumption record collapsed into its own
  selection and the tally was structurally pinned at zero.
- `--opt-report` schema → **2**. `selected` keeps its meaning but explicitly
  stops implying emitted bytes, so this is a meaning change, not an additive
  field. The text report grows a "Selected but NOT consumed" section.
- The census gains a `ptr-shape-consumed` column with its own floors, its own
  liveness minimum in `LIVENESS_FLOORS`, and three new failure modes:
  consumption recorded outside the selected population, consumption counted per
  access site instead of per value, and **wasted promotions that name no
  mechanism** — the last is what makes deleting a drop-recorder visible.
- `CONSUMPTION_INSTRUMENTED` lives in the script, not the regenerable baseline.
  Only `ptr-shape` is instrumented; the other seven keys report *no consumption
  data* rather than a zero, because "uninstrumented" and "never applied" are the
  exact pair this census exists to keep apart.

No existing floor was lowered; `batch`'s ratcheted `ptr-shape: 2` is untouched
and is now paired with `ptr-shape-consumed: 1`.

## Verification

- **Byte-neutral.** 23/23 workloads emit byte-identical objects with the report
  off vs on, and 23/23 between the pre-change and post-change compilers with the
  report off.
- **Ground truth is IR, not a counter.** Every consumed/unconsumed verdict was
  checked against an independent oracle: compile each workload with
  `PERRY_PTR_SHAPE_LOCALS=0` and with the default and hash the emitted object.
  The census agrees with it on all five promoting workloads, including the one
  case where the oracle is a superset (`09` differs only by dead clones).
- **Sabotage-verified in both directions**, all arms checking the *harness's*
  exit code: dropping `outcome` from `dedup_key`, removing all six consumption
  recorders, removing the context-drop recorder, removing the scalar-replacement
  recorder, counting per access site, folding proven-`this` consumption into the
  local column, and deleting the consumed liveness minimum each turn the gate
  red; the unmodified tree is green. The five pre-existing `PERRY_*_LOCALS=0`
  sabotages still go red, and CI's sabotage step now additionally asserts that
  `ptr-shape-consumed` tracks the compiler.

## Not measured

Consumption is instrumented for `Ptr<Shape>` only. Canonical i32/u32 moves the
storage, so consumption is structural there; canonical `Str` is a proof-only rep
and can be selected-and-unconsumed exactly like `Ptr<Shape>`, but its consumers
are the string-op lowerings and were out of scope. Phase 5a's proven-`this`
receiver is consumed but never selected at all, so it is reported separately and
excluded from the column rather than folded in.
