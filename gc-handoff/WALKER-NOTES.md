# #7984 — the fp-chain walker and the unwinder disagree by 96 bytes on aarch64 ELF

Working notes. Written as the investigation runs, so the dead ends are here on
purpose: three of them are the ones the panic message could not distinguish,
and knowing they are *excluded* is most of the value.

## The claim, restated exactly

On `ubuntu-24.04-arm`, `PERRY_STACKMAP_WALKER=verify` on `01_nursery_churn`:

```
fast walk visited 1 unique slots, unwinder visited 1
  left:  [281474742909688]     <- fp_chain
  right: [281474742909592]     <- unwinder
```

`left - right = +96`. Same slot count, so this is **not** a missed or invented
frame: it is one root resolved against two different bases.

## What the two walkers actually compute

Both are in `crates/perry-runtime/src/gc/roots/stack_maps.rs`. For the frame a
record belongs to:

| root's base register | `fp_chain` | `unwind` |
|---|---|---|
| DWARF 29 (fp) | `caller_fp`, the word at `[fp]` of the callee's frame record | `_Unwind_GetGR(ctx, 29)` |
| DWARF 31 (sp) | `caller_fp - fp_to_sp_offset(record.function_address)` | `_Unwind_GetCFA(ctx)` |

`fp_to_sp_offset` decodes the owning function's prologue: the immediate of
`add x29, sp, #imm` plus every `sub sp, sp, #imm` in the *contiguous run*
immediately after it (#7328, #7394).

Everything else — the parsed map, `match_records`' ±16 window, the record's
`(dwarf_reg, offset)` — is **shared**. A parse bug would move both walkers by
the same amount and produce no divergence at all. So the divergence is provably
one of:

- (a) `caller_fp` ≠ `_Unwind_GetGR(29)` for that frame,
- (b) `caller_fp - fp_to_sp_offset(F)` ≠ `_Unwind_GetCFA()` for that frame,
- (c) the two walkers attributed the record to *different frames*.

## Measured: which registers the roots actually use

Compiled the failing probe on macOS, took the pre-`opt` module out of
`--trace llvm`, and ran perry's own RS4GC pipeline plus `llc` for both triples:

```
opt -passes='function(mem2reg),rewrite-statepoints-for-gc' -S _01_nursery_churn_ts.ll
llc -mtriple=aarch64-unknown-linux-gnu -mcpu=neoverse-n2 -O3   # ELF
llc -mtriple=arm64-apple-macosx14.0.0  -mcpu=apple-m1    -O3   # Mach-O
```

`llvm-readobj --stackmap` over both objects:

| | ELF | Mach-O |
|---|---|---|
| root locations | **all `Indirect [R#31 + off]`** | **all `Indirect [R#31 + off]`** |
| functions with records | 2 (stack sizes 96, 176) | 2 (stack sizes 112, 192) |
| the single `[R#31 + 8]` root | the anon-shape constructor, records at +320 and +588 | same |

So **both platforms take the SP path**, and 96 is exactly the ELF
constructor's `stack size`. The Mach-O twin's frame is 112, which is why the
number is 96 and not something else — it is a property of that one frame.

## Measured: the prologues

ELF (the anon-shape constructor — the frame the `[R#31 + 8]` root lives in):

```
sub sp, sp, #96
str d10, [sp, #16]
stp d9,  d8,  [sp, #24]
stp x29, x30, [sp, #40]      <- frame record in the MIDDLE of the frame
str x23, [sp, #56]
stp x22, x21, [sp, #64]
stp x20, x19, [sp, #80]
add x29, sp, #40             <- x29 - body_sp = 40
.cfi_def_cfa w29, 56         <- CFA = x29 + 56 = body_sp + 96
```

Mach-O, same source function:

```
sub sp, sp, #112
... spills ...
stp x29, x30, [sp, #96]      <- frame record at the TOP of the frame
add x29, sp, #96             <- x29 - body_sp = 96
.cfi_def_cfa w29, 16
```

`fp_to_sp_offset` decodes **40** and **96** respectively, and both are correct.
An audit script that re-implements the decoder over assembly text and compares
it against a full simulation of every prologue `sp` adjustment reports **0
mismatches / 12 fp functions** across the generated module on both triples, and
0/14 over a hand-built C corpus (small frames, >4 KiB frames needing
`sub sp, sp, #N, lsl #12`, and multi-instruction allocations).

So (b) is **not** a prologue-decode error for this function. Hypothesis
"the contiguous-run rule missed a `sub sp, sp, #96`" is **refuted for the
observed frame**.

## Measured: `_Unwind_GetCFA`, and the frame-record-in-the-middle geometry

Standalone differential harness (`global_asm!` frames with hand-chosen layouts
plus the real `fp_to_sp_offset`), run on **macOS aarch64** and on **aarch64
Linux (Debian bookworm, libgcc) under colima**:

| frame shape | fp-chain sp | `_Unwind_GetCFA` | truth |
|---|---|---|---|
| ELF-shaped: record at `body_sp+40` of a 96-byte frame | exact | exact | — |
| Darwin-shaped: record at `body_sp+80` of a 96-byte frame | exact | exact | — |

`_Unwind_GetCFA` inside an `_Unwind_Backtrace` callback returns the **body
stack pointer of the frame whose return address `_Unwind_GetIP` reports** on
both implementations, which is what `stack_maps_unwind_contract.rs` asserts
(#7392) — confirmed independently here on aarch64 Linux.

That contract has, as far as CI goes, never run on this target: `cargo-test`
runs on `ubuntu-latest` (x86-64), so `unwind_cfa_is_the_frames_stack_pointer`
has no aarch64-Linux arm. Since the failing runner is Ubuntu 24.04 and the
first measurement above was Debian bookworm, the same binary was re-run against
**both** libgcc lines to rule out a difference:

| libgcc | CFA vs the frame's real body SP |
|---|---|
| `12.2.0-14+deb12u1` (bookworm) | exact, every frame |
| `14.2.0-4ubuntu2~24.04.1` (noble) | exact, every frame |

So the unwinder half of the contract holds on the failing distro's own
unwinder.

A second harness added a **frameless** intermediate frame (saves `x30`, never
establishes `x29` — legal on Linux, and what any C library built without
`-fno-omit-frame-pointer` emits; not legal on Darwin, where the ABI requires
the chain). Result: the fp chain does pair a return address in the frameless
function with a *different* frame's `x29`, but `fp_to_sp_offset` returns `None`
for a function with no `add x29, sp`, which makes the real walker `return None`
and `verify` panic with "fast walk unavailable" — a different message. Frames
either side of the frameless one still resolve **exactly** in both walkers.
So hypothesis (c) via a frameless frame is **refuted as a producer of this
message**; it produces the other one.

## ANSWER: the unwinder is right, and the fp-chain walker was blind twice

Reproduced end to end on aarch64 Linux (Ubuntu 24.04 arm64 under colima, LLVM
22.1.8 from apt.llvm.org, `--profile perry-dev`, the issue's own RUSTFLAGS).
The default `-mcpu=native` on that host resolves to an Apple core without SVE
and **passes** — all 14 probes, all of `verify`. Forcing the tuning the GitHub
ARM runner gets reproduces it:

```
PERRY_TARGET_CPU=neoverse-n2   -> diverges
PERRY_TARGET_CPU=neoverse-n1   -> passes
```

That is the whole "why only this runner": Perry tunes a host build with
`-mcpu=native`; a Neoverse-class core turns SVE on, and SVE changes the shape
of the prologue LLVM emits. No Apple arm can ever see it, and no x86-64 arm
either.

### The frame

`main` — the module body, 100 stack-map records — built `-mcpu=neoverse-n2`,
read out of the binary with `objdump -d` (`PERRY_DEBUG_SYMBOLS=1`, which
suppresses the final strip):

```
124790: fc180fea  str   d10, [sp, #-128]!
124794: 6d0123e9  stp   d9, d8, [sp, #16]
124798: a9027bfd  stp   x29, x30, [sp, #32]
12479c: 910083fd  add   x29, sp, #0x20      <- fp established; decoder reads 32
1247a0: a9036ffc  stp   x28, x27, [sp, #48] <- NOT a `sub sp`: the run ENDED here
1247a4: a90467fa  stp   x26, x25, [sp, #64]
1247a8: a9055ff8  stp   x24, x23, [sp, #80]
1247ac: a90657f6  stp   x22, x21, [sp, #96]
1247b0: a9074ff4  stp   x20, x19, [sp, #112]
1247b4: d10143ff  sub   sp, sp, #0x50       <- 80 bytes, DROPPED
1247b8: 043f57df  addvl sp, sp, #-2         <- 2 x VL more, DROPPED
```

Two independent defects, both in `fp_to_sp_offset`:

1. **A callee-save store ended the accumulation run.** It does not move sp, so
   it says nothing about whether the prologue's stack adjustments are over —
   but the rule was "the first instruction that is not a `sub sp` ends the
   run". LLVM interleaves those stores with the frame-pointer setup whenever
   SVE is on, so the 80-byte local allocation behind them was dropped.
2. **`addvl sp, sp, #-N` was not decoded at all.** Its unit is the runtime SVE
   vector length, which is not in the instruction.

### Which walker is right, arithmetically

From the report the new instrument prints, under qemu `-cpu max` (VL = 64 B):

```
fp-chain: [0x400000800af0, 0x400000800b08]
unwinder: [0x400000800a20, 0x400000800a38]
same slot count, so this is a base disagreement, not a missed frame;
  fp-chain minus unwinder = [208, 208] byte(s)
frames visited: fp-chain 9, unwinder 10; records matched: fp-chain 1, unwinder 1
  slot 0x400000800af0 = base 0x400000800ab0 +64 | ip ... (fn 0xaaaaaabc4790 + 0x22c)
    fp_to_sp_offset(fn) = Some(32), prologue words: fc180fea 6d0123e9 a9027bfd
      910083fd a9036ffc a90467fa a9055ff8 a90657f6 a9074ff4 d10143ff
```

Same function, same ip, same record, same offsets — so it is a base
disagreement, and the bases are `0x…ab0` (fast) and `0x…9e0` (unwinder).

    caller_fp        = fast base + decoded  = 0x…ab0 + 32   = 0x…ad0
    true x29-body_sp = 32 + 0x50 + 2*64                     = 240
    true body_sp     = 0x…ad0 - 240                         = 0x…9e0   <- the unwinder's base

**The unwinder's base is the frame's real body SP, derived independently from
the prologue the fast walker misread.** That is the proof, not an appeal to the
unwinder being the reference implementation.

### 96 is not a constant

It is that frame's missed tail, and it scales with the vector length:

| host | missed | = |
|---|---|---|
| `ubuntu-24.04-arm` (Cobalt 100, VL = 16 B) | 96 | 0x50 + 1 x 16 |
| qemu `-cpu max` (VL = 64 B) | 208 | 0x50 + 2 x 64 |

So the issue's open question 2 — "is 96 constant?" — is answered: no, and a fix
keyed on 96 would have been wrong on every other vector length.

### The fix

1. Stores through sp with no writeback (`stp`/`str`, enumerated by opcode) are
   transparent to the accumulation run. Unrecognised instructions still end it,
   so the safe direction is preserved.
2. `addvl`/`addpl` writing sp is decoded: the multiplier from `imm6`, the unit
   from `prctl(PR_SVE_GET_VL)`, cached. Where the length cannot be read — every
   core without SVE, including all Apple ones — the whole decode fails and the
   frame goes to the platform unwinder, which reads DWARF CFI and needs no VG
   for an fp-based frame.

Fail-closed matters here in a way that a "return the part I could read" fallback
would not: reporting 0x70 for a frame whose body SP is 240 bytes down is exactly
the silent wrong answer this bug is.

## Reproduction recipe, for the next person


Nothing here needs an aarch64 Linux host except the last line:

```bash
# 1. the module IR, pre-RS4GC
PERRY_RS4GC=1 perry benchmarks/gc_ratchet/probes/01_nursery_churn.ts -o /tmp/p --trace llvm
# 2. perry's own pipeline, then either backend
grep -v '^module asm' .perry-trace/llvm/_01_nursery_churn_ts.ll > m.ll   # drops a Mach-O-only .no_dead_strip
opt -passes='function(mem2reg),rewrite-statepoints-for-gc' -S m.ll -o rs.ll
llc -mtriple=aarch64-unknown-linux-gnu -mcpu=neoverse-n2 -O3 rs.ll -o linux.s
clang --target=aarch64-unknown-linux-gnu -mcpu=neoverse-n2 -c linux.s -o linux.o
llvm-readobj --stackmap linux.o          # every root's base register and offset
# 3. an arm64 Linux shell, for the unwinder half
colima start --arch aarch64 && docker run --rm --platform linux/arm64 -v "$HOME/x:/x" rust:1-slim-bookworm ...
```
