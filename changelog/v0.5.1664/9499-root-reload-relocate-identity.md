### Fixed

- **`map.set(key, () => …)` no longer stores a completely different live
  object as the key.** On the MCP SDK handshake, `this._responseHandlers.set(messageId, response => { … })`
  stored the whole `jsonrpcRequest` object; `_responseHandlers.has(messageId)`
  was then false and every request timed out (#9485). The map *worked* — with
  wrong contents, no error and no diagnostic.

  The map lowering is not at fault, and neither is the rooting order: the
  emitted IR pushes the key into a temp-root slot **before** the closure
  literal is lowered and re-reads it from that slot **after** the closure's
  allocation, which is exactly the contract. The defect is one layer down, in
  the native-root (RS4GC) slot lowering, and it is not a collection-timing bug
  at all — `PERRY_GC_SCAVENGE=off`, `PERRY_GC_MOVING_LOOP_POLLS=0` and
  `PERRY_GC_SCAVENGE_NURSERY_MB=1` all reproduce it identically, while
  `PERRY_RS4GC=0` is correct.

  A root slot is a `ptr addrspace(1)` alloca, so the value read back out of one
  is a GC pointer. `function/precise_roots.rs` spelled that reload as a bare
  `ptrtoint`, which InstCombine composes with the *next* root push's `inttoptr`
  and folds away — making one `ptr addrspace(1)` SSA value the statepoint
  operand at two safepoints, with a **hole** in between where the value sits in
  a plain non-root alloca. LLVM's statepoint lowering assumes the opposite
  (`findPreviousSpillSlot`: "spill location is known for gc relocates"), so at
  the second safepoint it re-reads the first safepoint's stack slot without
  re-storing — and that slot has been recycled for an intervening statepoint's
  operand. Measured on the reduced fixture: the key handed to `js_map_set` was
  the closure allocated three lines earlier, read out of `-0x50(%rbp)`, a slot
  nothing had written since the unrelated closure that landed there.

  Every root reload now passes through an identity barrier — an empty `asm`
  with a tied `"=r,0"` operand, which emits no machine instruction — so
  re-rooting a value mints a fresh statepoint operand with its own spill slot
  instead of re-identifying it with a relocation from an earlier safepoint.
  `freeze i64` is the prettier spelling and was tried first; it is rejected on
  evidence, not taste: with `freeze` the reduced fixtures still pass but the
  real MCP SDK client throws `TypeError: value is not a function` on every run,
  where the `asm` form connects. The
  trigger needs the key to have been rooted once already and released — the
  common `const messageId = this._next++` shape — so `set(messageId, …)` after
  a `++` was affected while `set(messageId + 0, …)` and `set(messageId, 7)`
  were not.

  `test-files/test_gap_9499_rooted_key_across_closure_alloc.ts` is the
  deterministic, knob-free reproduction with its three controls.
