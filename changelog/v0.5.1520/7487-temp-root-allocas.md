### Codegen: temp roots lowered onto pooled frame allocas (#7469)

The #6951 expression-temporary rooting contract cost three runtime calls per
protected temporary — `js_gc_temp_root_push`, a mandatory re-read, truncate —
and after the hot-TLS consolidation those three calls were 206 of the 542
remaining `_tlv_get_addr`-attributed profile samples on `churn.ts`. Named
locals already demonstrate the cheap form of the same root: an entry alloca
bound to a shadow-frame slot, written and re-read with plain stores and loads,
upgraded by the RS4GC/stack-map lowering into a relocated `addrspace(1)` slot
the collector rewrites. A temp needs nothing a local doesn't.

`TempRootPool` (per-function, compile-time bookkeeping only) lowers the same
API onto that mechanism: push = store + the identical bind/root-shading
emission a named local's store uses (`emit_shadow_slot_bind_ptr`, extracted
from `emit_shadow_slot_bind_for_local`); get = load; truncate = zero the slot,
clear its frame mirror, and release the pool entries at and above it — the
same drop-everything-above discipline the FFI stack imposed, so no caller can
observe the difference. Handles keep their `String` currency, so every caller
(`RootedOperands`, `StoreOperandGuard`, `rooting.rs`'s `call_rooted`) compiles
unchanged. Frame slots are reserved on demand through `reserve_shadow_slot`,
whose in-place slot-count rewrite is what rules out the #7184
out-of-frame-bounds shape; the store-then-bind order at the push site is the
#7192 dominance invariant, stated at the emission site. When shadow-stack
emission is off the whole function falls back to the FFI stack byte-for-byte.

`churn.ts`'s hot function drops from 9 temp-root FFI calls to 0. Validation:
the #6951/#6971/#7200/#7211 reproducer shapes match Node under default GC,
under `PERRY_CONSERVATIVE_STACK_SCAN=off` (the mode where an unrooted temp is
a live use-after-free), and under `PERRY_GC_ZEAL=1` +
`PERRY_GC_PROTECT_FROMSPACE=1` with `PERRY_GC_MOVING_LOOP_POLLS=1` — with the
instrument proven live (5 quarantined from-space sets per run, so survivors
genuinely moved and the slots were genuinely rewritten). The
`gc_root_dominance` corpus passes with the empty allowlist (0 violations,
2,379 functions / 9,145 root stores) and the stale-register budget improves to
21 of 39. GC ratchet vs `main`: collector accounting identical on all 12
probes; `09_try_catch_roots` retains **14% less** heap — frame-slot temps are
zeroed at release instead of lingering on the runtime stack until a later
truncate.
