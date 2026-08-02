### Fixed

- **The generic dynamic-call lowering no longer holds its callee, its `this`
  receiver or its already-lowered arguments in bare registers across the
  argument list.** `js_closure_callN` is the central dispatch path — `f(g())`,
  `o.m(g())`, `curry(1)(2)`, every call whose callee is a value rather than a
  statically resolved function — and it held **three** classes of GC value in
  SSA registers across work that can collect. This is the site #7206 named and
  deliberately left open, and the last known instance of #7192's
  root-store-dominance class.

  An SSA register is not a GC root. Under `PERRY_GC_MOVING_LOOP_POLLS=1` a
  back-edge poll inside an argument runs an evacuating minor: each held value
  *survives* — the capture cell, shadow slot or module global it was read from
  is a root — and therefore **moves**. The collector rewrites that location;
  the register keeps naming from-space.

  - **the callee**, held across the whole argument list. The checked unbox then
    masks a pre-move address and `js_closure_callN` reads a closure header out
    of abandoned memory: `TypeError: value is not a function`, the failure
    shape #7154 has worn since #7184.
  - **the `this` receiver**, held across the read of the callee off it *and*
    the argument list. #7206 fixed this operand on the sibling
    `js_native_call_method_by_id` dispatch; this is the same operand on the
    generic one — the dispatch a closure-valued property takes (hono's
    `RegExpRouter.match = match`, the #519 shape).
  - **each already-lowered argument**, held across the arguments after it *and*
    across the rebind unbox.

  The three live windows are different, so they are computed separately rather
  than protected as one block:

  | operand | window |
  |---|---|
  | receiver | the callee read + every argument |
  | callee | every argument |
  | argument *i* | the arguments after *i* + the rebind unbox |

  That last window is the subtle one, and it is why this could not be a
  copy of #7206's fix. `js_closure_unbox_callee_checked_rebind` calls
  `clone_closure_rebind_this`, which **allocates** a replacement closure
  (`closure/dynamic_props.rs:1040`) when the callee captures `this`. It sits
  *below* the last argument and *above* `js_closure_callN`, so the arguments
  are re-read below it — hence `RootedOperands::reread_one`, which re-reads one
  operand at a caller-chosen point instead of re-reading the whole group at
  one. Hoisting the unbox above the argument list would remove the window
  instead, but its throw is observable and the spec evaluates arguments before
  it. For the >16-arity path the argument stores into the stack buffer moved
  below the unbox for the same reason: a stack buffer is not a root, so filling
  it above an allocating rebind just freezes pre-move addresses one indirection
  further out.

  The receiverless path takes `js_closure_unbox_callee_checked`, which is a tag
  check and a mask and allocates nothing, so `f(x, y)` on inert operands emits
  exactly the IR it emitted before. Temp roots, not re-lowering: re-lowering
  the callee or receiver would observe an assignment made by an argument, which
  is a miscompile rather than a rooting fix.

  Three new gap tests, one per held value, each red on the parent under a
  genuine `POLLS=1` build and green after. **The flag is compile-time since
  #7161 *and* runtime-armed (`gc_moving_loop_polls_enabled()`,
  `gc/policy.rs:1759`) — setting only one of the two is a false green, and the
  first cut of these tests passed 10/10 for exactly that reason.**

  | | parent | this change |
  |---|---|---|
  | `test_gap_gc_closure_call_callee_rooting.ts`, `POLLS=1` | `TypeError: value is not a function` **10/10** | `bad 0` **10/10** |
  | `test_gap_gc_closure_call_this_rooting.ts`, `POLLS=1` | `TypeError: value is not a function` **10/10** | `bad 0` **10/10** |
  | `test_gap_gc_closure_call_argument_rooting.ts`, `POLLS=1` | `TypeError: value is not a function` **10/10** | `bad 0` **10/10** |
  | all three, `POLLS=1` + `PERRY_GEN_GC=0` | `bad 0` | `bad 0` 5/5 |
  | all three, default (no polls) | `bad 0` | `bad 0` 5/5 |

  **Cost, measured over the 141-module `sfw-registry` corpus** — this is the
  hottest call path in the compiler's output, so it was measured rather than
  assumed. The `operand_protection` gate means an operand whose window cannot
  collect emits nothing at all, which is why the delta is this small:

  | | before | after | delta |
  |---|---|---|---|
  | linked binary | 39,216,688 B | 39,233,200 B | **+0.042 %** |
  | emitted IR | 1,999,570 lines | 2,001,607 lines | **+0.10 %** |
  | `js_gc_temp_root_push` sites | 8,394 | 8,885 | +491 |

### Changed

- **`scripts/gc_root_dominance_check.py`: `js_closure_unbox_callee_checked` is
  now in `NONCOLLECTING`**, citing `closure/unbox.rs:25` — it is a tag check on
  the NaN-boxed callee and a low-48 mask, with no allocation, no user code and
  no poll. It sits between every dynamic call's last argument and its
  `js_closure_callN`, so its absence reported the entire argument list of every
  1-arg dynamic call as stale: **372 of the 729 fatal-sink hits were this one
  false positive**, all of them marked `MOVING: no`. This is the checker's
  stated one-sided discipline working as designed — a missing entry costs false
  positives, never a missed bug — and it is why the raw before/after counts
  below are quoted against the corrected list.

  `js_closure_unbox_callee_checked_rebind` is deliberately **not** added: it
  allocates, and the fix above depends on it counting as a collection point.

## Verification

Over the 141-module `sfw-registry` corpus, fatal-sink slice, with the corrected
`NONCOLLECTING`: **231 → 205**.

`cargo test -p perry-codegen`: failing set **identical to the parent** —
6 `loop_safepoint_purity` (#7161's default flip), 16 `native_proof_regressions`,
3 `native_proof_buffer_views`, 1 `shadow_slot_hygiene`, 1
`typed_shape_descriptors`, all pre-existing and measured directly on the parent
commit rather than assumed. One `perry-codegen` lib unit test that is red on the
parent passes here. The bind-anchored gate reports the same single non-moving
residual #7192 left (`js_closure_alloc_with_captures_singleton`, 0
moving-reachable).

## What this does NOT close

**`sfw-registry --help` under a genuine `POLLS=1` build is still red, so the
stopgap from #7161 stays.** Measured on this build, compiled *and* run with the
flag: **3/10 pass, 7/10 SIGSEGV**. Its default arm is clean **10/10**, so
nothing was traded away. The three fixed registers were real and are now
provably rooted, but they are not the last thing standing between the registry
and a clean evacuating minor.

Two concrete leads for whoever picks this up, both found while fixing the above
and neither speculative:

1. **`prev_this` in the same lowering is the same bug, unfixed.**
   `js_implicit_this_set` returns the *previous* implicit `this`, read out of
   the `IMPLICIT_THIS` cell — which `object/this_binding.rs:176` documents as a
   scanned mutable root the collector rewrites. That value is then held in a
   bare register across the entire user call and written back afterwards, so a
   collection anywhere inside the callee makes the restore publish a from-space
   pointer back into a root. It is invisible to the current checker on both
   ends: `js_implicit_this_set` is not in `ROOT_READ_CALLS`, and it is not a
   `RECEIVER_SINKS` fatal sink. Fixing it costs a temp root on every dynamic
   call, which is why it was measured and left rather than folded in here.
2. **205 fatal-sink hits remain**, no longer dominated by any single class —
   37 `js_closure_call1`, 22 `js_closure_call2`, 18
   `js_closure_call_apply_with_spread`, 17 `js_array_spread_append`, 15
   `js_object_set_field_by_name`, 15 `js_array_concat`, and a long tail. The
   spread path (`expr/call_spread.rs`) is the obvious next one: it is the same
   dispatch family and was never touched by #7206 or this change.

Refs #7154, #7206, #7192, #7198, #7184, #7161, #7114, #6951, #519.
