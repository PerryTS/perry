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

## Where that leaves it

Every mechanism reproducible off the target agrees. The divergence needs the
real `ubuntu-24.04-arm` binary, and the gate that found it could not say which
walker was wrong.

**Step one (PR #7997)**: make the gate say. Both walkers now report a
`ResolvedRoot` — address, the frame return address it was matched on, the
record's function, the map's base register and offset, and the base that walker
resolved that register to — and `verify` prints all of it, calls an
equal-slot-count disagreement a *base* disagreement rather than a missed frame,
and on aarch64 dumps `fp_to_sp_offset`'s decode plus the prologue words it
read. That is enough to settle (a) vs (b) vs (c) from one CI run.

Gate on the report itself: the prologue dump only runs for a function address
the parsed map vouches for (`function_starts`). Reading instructions from an
address supplied by the data under suspicion is how a diagnostic becomes a
SIGSEGV with no output — measured, in the first draft of this file's tests.

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
