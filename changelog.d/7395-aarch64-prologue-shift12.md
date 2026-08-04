### Fixed

- **GC (aarch64): the stack-map fast walker enumerated the wrong stack for any frame ≥ 4 KiB (#7394).**
  `fp_to_sp_offset` decodes a generated function's prologue to recover its body
  stack pointer. Its `add x29, sp, #imm` / `sub sp, sp, #imm` patterns masked in
  bit 22 — the `sh` field, which selects `lsl #12` on the immediate — so an
  instruction using the shifted form did not match the opcode comparison at all.

  LLVM switches to `lsl #12` the moment a frame needs 4 KiB or more, which a
  generated function crosses routinely: each string-concat chain spills a
  `[32 x double]` buffer, and one gap-test binary contains **80** functions with
  a shifted frame adjustment. The measured prologue of
  `perry_fn_test_gap_gc_call_argument_rooting_ts__run`:

  ```
  9101c3fd   add x29, sp, #0x70          ; fp established
  d14007ff   sub sp, sp, #0x1, lsl #12   ; sh=1 — no match, run ENDS here
  d12103ff   sub sp, sp, #0x840          ; never reached
  ```

  The damage is worse than the dropped term: because the shifted `sub` failed to
  match, it also terminated #7328's contiguous-`sub` accumulation run and the
  `sub sp, sp, #0x840` behind it was dropped too. The decoder reported `0x70`
  for a frame whose body SP is `0x18B0` below the frame pointer, so the walker
  enumerated slots **6208 bytes off** — and evacuation *writes* through the
  slots it is handed. Live roots were missed and unrelated stack words rewritten.

  This is reachable in the shipping configuration: RS4GC/statepoints is the
  default root backend wherever the runtime can walk frames, and the fast x29
  chain is the default walker. `test_gap_gc_call_argument_rooting` printed
  `bad 1` — a wrong answer, not a latent bad pointer — under nothing but
  `PERRY_GC_HEAP_LIMIT=8`, with the conservative stack scan left ON; the
  equivalent `PERRY_RS4GC=0` build printed `bad 0` while evacuating 6344
  objects in the same run.

  `immediate_of` now decodes `sh` for both the `add` and the `sub` forms. Four
  decoder unit tests cover the shifted `sub`, the shifted `add`, and the
  measured two-`sub` prologue that motivated the fix.

  Not fixed here, and filed separately: the `PERRY_STACKMAP_WALKER=unwind`
  fallback derives its SP-relative base from `_Unwind_GetCFA` minus the
  recorded stack size and disagrees with the (now correct) fast walk by exactly
  one frame size, so `PERRY_STACKMAP_WALKER=verify` still trips.
