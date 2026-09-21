# #10863 — prediction, committed BEFORE the fix arm is built

Tree: upstream/main f88acdaa1 (v0.5.1631). Host: perrymaster. All arms link the
PREBUILT runtime (`auto-optimize: failed to spawn cargo ... using prebuilt
libraries` in every build log), so both arms are the same runtime MODE.

## 1. Measured BEFORE (base arm, no patch)

Marginal instructions per read, `perf stat -x, -e instructions:u`, min of 3,
fitted between N=500k and N=5M, with a 2M point to prove flatness. perry is
AOT: all four fixtures are FLAT to 0.1 instr/read.

| fixture | hot key | i(0.5M) | i(2M) | i(5M) | marginal | lo/hi |
|---|---|---|---|---|---|---|
| m64       | overflow, 64 shapes, 1 inline shape recurs 1/64   | 431569815 | 1568535182 | 3842495404 | **757.98** | 757.98 / 757.99 |
| m64arm    | overflow, 64 shapes, inline armer recurs 1/4096   | 441061168 | 1600327271 | 3919220848 | **772.92** | 772.84 / 772.96 |
| m64noarm  | overflow, 64 shapes, NO shape ever inline         | 434422558 | 1575100054 | 3854282910 | **759.97** | 760.45 / 759.73 |
| m64inline | inline (control, latch already works)             | 727791657 | 2757058355 | 6815615110 | **1352.85** | 1352.84 / 1352.85 |

`PERRY_IC_DIAG`, N=20M (1 s snapshot):

* m64:       primes=18025216 same=0 new=100.0% in_ways=0 | fresh=2 armed=18025214 **megamorphic=0** | own_inline_primed=1 own_overflow_primed=18025215
* m64arm:    primes=18211328 in_ways=0 | fresh=2 armed=18211326 **megamorphic=0** | own_inline_primed=1
* m64noarm:  primes=18600960 in_ways=0 | **fresh=18600960 armed=0** megamorphic=0 | own_inline_primed=0
* m64inline: primes=11644672 in_ways=0 | fresh=5632 armed=106989 **megamorphic=11532051 (99.03%)**

The issue reproduces exactly: megamorphic=0 on the overflow key, armed on 100%
of primes; the same rotation with an inline hot key latches on 99.0%.

### The cost of the armed way block, measured directly

`m64arm` and `m64noarm` are the SAME program; they differ only in whether the
1-in-4096 "armer" object carries the hot key inline (which arms the site) or
deep in overflow (which never arms it). Everything else — shape count, key
walk, loop, allocation — is identical.

  772.92 - 759.97 = **12.95 instructions per read**

That is what the emitted `pic.ways` block (a `PIC_WAY_STATE>0` branch, 8
dependent loads, 4 compares and the select tree) costs on a site where it can
never hit. It is 1.68% of a 773-instruction read, NOT 37%: in these fixtures
every read already misses into the full `js_object_get_field_ic` handler
(~745 instr), so the block is a small slice of a large read. #7753's +37% was
against a read the cache could otherwise serve. I predict in advance that I
will NOT reproduce +37%, and that quoting it as the win would be wrong.

## 2. The fix I am about to build

`pic_prime_get`: the cascade decision and the LATCH decision currently share
one `if !cascade { return; }`. Separate them. When a cascade is suppressed
*only* because the evicted MRU slot is overflow-encoded, and the site is armed
(`state > 0`, the exact predicate the emitted gate evaluates), advance the
consecutive-eviction run and latch at `PIC_MEGAMORPHIC_EVICTIONS` exactly as
the no-free-way arm does. NOTHING is written to a way on that path: the
encoded slot stays out of the ways, which is #9287's requirement and is not in
dispute.

## 3. COUNTS I predict (these settle it, not the cost)

### m64 (the filed repro), PERRY_IC_DIAG, N=20M

The site re-arms only through the ONE inline shape (shapes[0], z at inline slot
1). Steady-state cycle after the fix:

* 16 `armed` primes — the run climbing 1..16 after a re-arm, then the latch;
* 2048 `megamorphic` primes — the PIC_LATCH_RETRY countdown;
* ~32 `fresh` primes at state 0, waiting for the one shapes[0] -> shapes[1]
  transition per rotation that can cascade an inline slot and re-arm.

cycle ~= 2096 primes ->

| counter | before | predicted after | accept if |
|---|---|---|---|
| way_state megamorphic | 0 (0.0%) | ~2048/2096 = **97.7%** | > 90% |
| way_state armed | 18025214 (100.0%) | ~16/2096 = **0.76%** | < 2% |
| way_state fresh | 2 | ~32/2096 = **1.55%** | < 5% |
| in_ways | 0 | **0** | == 0 |
| way-slot encoded-bit counter | 0 | **0** | == 0 (hard requirement) |
| own_inline_primed | 1 | ~1.5% of primes (shapes[0] stops hitting a way while latched) | > 1000 |

### m64arm: same shape, but re-arms only 1/4096 -> megamorphic > 99%, armed < 0.5%.
### m64noarm: UNCHANGED (fresh=100%, armed=0) — the site never arms, so the new
    arm is never taken. Any change here means I armed something I should not have.
### m64inline (control): UNCHANGED within 10% (fresh ~1, armed ~19, mega 2048
    per cycle). The new arm requires prev_is_overflow, which never holds there.

## 4. COST I predict

Derived from the 12.95 instr/read block cost measured above, not guessed.

| fixture | base | predicted after | predicted delta | reasoning |
|---|---|---|---|---|
| m64arm    | 772.92 | **759.5 - 761.5** | **-11.4 .. -13.4 (-1.5% .. -1.7%)** | the block goes away on ~99.5% of reads; it should land on the m64noarm control (759.97). This is the decisive cost fixture. |
| m64       | 757.98 | **748 - 756** | **-2 .. -10 (-0.3% .. -1.3%)** | the block goes away (-12.8) BUT the one way that currently serves shapes[0] on 1/64 of reads is zeroed by the latch, so those reads become misses: +(M0-13)/64, and M0 (a 2-key object, z found on the first reverse probe) is close to the ~700 miss floor, i.e. +10.7. Net is small ON THIS FIXTURE BY CONSTRUCTION. |
| m64noarm  | 759.97 | **759.97 +- 1** | 0 | control: never armed, nothing to remove. |
| m64inline | 1352.85 | **1352.85 +- 4 (0.3%)** | 0 | control: already latches. |
| own3 / w4 / inh / inh3 | see mainwatch | no regression | <= +1% each | |

## 5. What it means if I get the count but not the cost

* Counts move as predicted (megamorphic 0 -> >90%, armed 100% -> <2%) and
  m64arm does NOT fall by at least 8 instr/read: the emitted pic.ways block is
  not costing what the direct m64arm-vs-m64noarm measurement says, i.e. the
  A/B arms are not actually distinct or the latch is not reaching the gate.
  Check the arms by content first (binary hash, diag counters); if the arms
  ARE distinct and the counts DID move, then report the fix as a
  correctness-of-policy fix with a measured cost near zero and say so plainly.
  Do NOT quote #7753's +37%.
* Cost moves but counts do not: the change did nothing and something else
  moved. Reject the arm and find out what.
* m64noarm or m64inline moves at all: I have armed or latched a site the new
  arm should never touch. Reject.
* Any nonzero encoded-bit-in-a-way count, ever: revert immediately. That is a
  wild load, not a regression.
