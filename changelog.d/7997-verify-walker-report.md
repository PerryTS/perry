### aarch64: the fast GC stack-map walker misread every SVE-shaped prologue (#7984)

`PERRY_STACKMAP_WALKER=verify` caught the frame-pointer-chain walker and the
Itanium unwinder resolving the same GC root 96 bytes apart on
`ubuntu-24.04-arm`. The fp-chain walker is the wrong one — it is also the one
that runs when `verify` is off — and it was blind twice in the same prologue.

Reproduced end to end on aarch64 Linux. The trigger is the tuning, not the
distro: Perry builds a host binary with `-mcpu=native`, and on a Neoverse-class
core that turns SVE on, which changes the shape LLVM emits. The same probe built
`-mcpu=neoverse-n1` passes; no Apple arm can see it at all. Measured on `main`
— the module body, 100 stack-map records — built `-mcpu=neoverse-n2`:

```
12479c: add   x29, sp, #0x20      <- fp established; the decoder read 32
1247a0: stp   x28, x27, [sp, #48] <- not a `sub sp`, so the run ended HERE
...     four more callee-save pairs
1247b4: sub   sp, sp, #0x50       <- 80 bytes, dropped
1247b8: addvl sp, sp, #-2         <- and two vector lengths more, dropped
```

1. **A callee-save store ended the accumulation run.** It does not move sp, so
   it says nothing about whether the prologue's stack adjustments are finished,
   but the rule was "the first instruction that is not a `sub sp` ends the run".
   Stores through sp with no writeback are now transparent to it, enumerated by
   opcode so an unrecognised instruction still ends the run — the safe
   direction.
2. **`addvl`/`addpl` writing sp was not decoded at all.** Its multiplier is in
   the instruction; its unit is the runtime SVE vector length, which is not.
   That is now read once via `prctl(PR_SVE_GET_VL)` — a syscall rather than
   `rdvl`, which faults on a core without SVE — and cached. Where it cannot be
   read the whole decode fails and the frame goes to the platform unwinder,
   which reads DWARF CFI and needs no VG for an fp-based frame.

**96 was never a constant.** It is that frame's missed tail and it scales with
the vector length: 96 on the runner (`0x50 + 1 x 16`), 208 under `qemu -cpu max`
at VL = 64 (`0x50 + 2 x 64`). A fix keyed on 96 would have been wrong on every
other vector length.

Validated at two vector lengths so the scaling is pinned rather than one
machine's answer: 28 `verify` runs (14 probes x VL 16 B and 64 B) under
`qemu-aarch64 -cpu max`, all byte-exact against the pinned Node oracle. The fast
walker is live there — `11_collect_at_depth` reports `fp_walks 12`,
`fallback_walks 0`, `records_matched 1338`, `locations_visited 2678` with
`verify` green, so 2678 slots were cross-checked against the unwinder and
agreed.

### `PERRY_STACKMAP_WALKER=verify` now names the disagreement it finds

All the gate could report was `fast walk visited 1 unique slots, unwinder
visited 1` and two addresses in decimal — not the frame, not the base register,
not the function whose prologue was decoded, and therefore not which walker was
wrong. Every candidate explanation predicts exactly that output.

Both walkers now hand back a `ResolvedRoot` rather than a bare
`MutableRootSlot`: the address, plus the frame return address it was matched on,
the record's function, the map's base register and frame offset, and the base
that walker resolved that register to. `visit_stack_map_root_slots` projects it
straight back, so the collector's view is unchanged. On a mismatch `verify`
prints every root from both walks, states that an equal slot count means a
*base* disagreement rather than a missed frame (with the per-slot delta), prints
each walk's frame and record counts so an early-terminating walk is
distinguishable, and on aarch64 dumps `fp_to_sp_offset`'s decode together with
the prologue words it read. That report is what identified #7984 in one run.

The prologue dump is gated on the parsed map vouching for the function address.
The first draft was not, and a unit test with a synthetic address turned the
diagnostic into a SIGSEGV with no output — which is what would happen in the
field for the one failure mode where a report matters most, a map whose
addresses are wrong.

### The two aarch64 walkers now have unit coverage at all

Nothing `cargo test` runs had ever called `fp_chain::visit` or `unwind::visit`;
the decoder and the matcher were covered, the step that turns a
`(register, offset)` pair into a stack address was not.
`stack_maps_walker_agreement.rs` drives both over `global_asm!` probe frames in
the two real layouts — the aarch64-ELF one with the frame record in the middle
of the frame, and the Mach-O one with it at the top — and requires each walker
to land on a word **holding a sentinel** the probe wrote into the slot its
record names. Set equality is satisfied by two empty sets and by two identically
wrong ones; the slot's contents are the discriminating quantity.
`a_wrong_frame_offset_is_caught` is the sabotage arm, and a third test pins that
an undecodable prologue declines the fast walk while the unwinder still
resolves the root — the fallback the SVE fix rests on.

`gc-native-roots.yml` runs them on both aarch64 arms, before the probe matrix,
requiring each test by name (`--lib <filter>` is a substring match, so a rename
would select nothing and cargo would still exit 0). Its crash path tailed 20
lines of the failing run's stderr, which truncates the report; now 120.
