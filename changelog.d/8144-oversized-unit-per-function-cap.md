**#8132 follow-up — measured NEGATIVE result: the oversized-unit opt-level
threshold is not retuned, and here is the evidence for why not.**

`oversized_opt_flag` routes a codegen unit past `PERRY_LL_O0_THRESHOLD_BYTES`
(6 MiB of IR) to `-Os` unless it looks like the `#4880` "one giant generated
function" pathology, in which case only `-O0` finishes in practical time. It has
two arms: an average bytes-per-function cap (256 KB) and a per-function arm for
the unit's widest body. The per-function arm did not have a threshold of its
own — it **borrowed `ll_o0_threshold_bytes()`**, a *whole-unit* constant
answering a different question. `next@16.3.0`'s bundled `jsonwebtoken` lowers to
a unit whose widest body is 3.5 MB, misses that 6 MiB cutoff, takes `-Os`, and
falls into the RS4GC blow-up #8132 describes.

The obvious fix is to lower the cap. Measured, it is a bad trade.

### There is no separating band

Compiling the 17 largest `next@16.3.0` `dist/compiled/**` bundles produced **52
oversized codegen units**. Their widest function runs **751 KB to 11.1 MB,
continuously** — deciles 0.73 / 1.3 / 1.6 / 1.9 / 2.1 / 2.2 / 2.7 / 3.4 / 3.8 /
5.4 MB. There is no gap between "large bundle of ordinary functions" and
"monolith", because in real webpack output *every* oversized unit is
monolith-bearing: the bundler emits each chunk's module factory as one function.
Every one of the 52 averages 19-67 KB/fn, i.e. all 52 clear the 256 KB average
cap, so the per-function arm is the only arm that ever fires on this shape.

| per-function cap | units routed to `-O0` |
|---|---|
| 256 KB (the average cap) | 52 / 52 (100%) |
| 1 MiB | 50 / 52 (96%) |
| 1.5 MB (needed to catch both #8132 units) | 44 / 52 (85%) |
| 2 MiB | 33 / 52 (64%) |
| 4 MiB | 10 / 52 (19%) |
| **6 MiB (today, unchanged)** | **3 / 52 (6%)** |

### The price of reclassifying, on the #8132 bundle itself

One compiler, one fixture (`dist/compiled/jsonwebtoken/index.js`, sha256
`056c2ddd…a6b9`), arms selected only by the new env var, `PERRY_RS4GC=0`
(see below for why):

| arm | `__text` | dylib total | clang CPU | wall |
|---|---|---|---|---|
| both units `-Os` (today) | 2,798,984 | 4,272,080 | 43.76 s | 48.6 s |
| widest arm at 2 MiB (one unit each way) | 4,294,952 | 5,775,232 | 29.61 s | 35.7 s |
| both units `-O0` | 5,452,344 | 6,931,088 | 16.49 s | 25.3 s |

**`-Os` → `-O0` is +94.8% `__text` and +62.2% total, to buy −62.3% clang CPU.**
That confirms the "30-50% less `__text`" figure the existing comment claims, and
it is the cost that would land on 85-96% of oversized units under any cap low
enough to help #8132. Dropping a unit to `-O0` takes its ~400 ordinary functions
down with the one monolith; that is the whole trade.

### And it would not fix #8132 anyway

On `origin/main` (`0a1e78e5f`, `perry-dev` profile, LLVM 22.1.4, arm64 macOS)
**neither arm produces a binary for that bundle with statepoints enabled.** Both
die with `SIGBUS` at the same site — `AArch64TargetLowering::LowerCall` under
`SelectionDAGBuilder::LowerAsSTATEPOINT` — reproduced 4/4: `-Os` at 292 s,
`-O0` at 120-208 s. It is not a stack overflow; the faulting address lands in
reserved, never-mapped space, and raising the codegen worker stack from the
default 2 MiB to 1 GiB changed nothing. The text backend fails differently
(`clang: unterminated attribute group` on the 414 MB rendered unit). Only
`PERRY_RS4GC=0` compiles it, in 20 s. So issue #8132's recorded `-O0` result
(2m46.5s, 40 MB dylib) does not reproduce on that commit, and routing the unit
to `-O0` swaps a hang for a crash rather than unblocking #8040.

### What this change does land

Nothing that alters a default:

- `DEFAULT_LL_O0_MAX_FN_BYTES` (6 MiB) gives the per-function arm its own
  constant instead of borrowing the whole-unit one, so raising
  `PERRY_LL_O0_THRESHOLD_BYTES` — a knob about unit *size* — stops silently
  moving the monolith cap too. Same value, same behaviour today.
- `PERRY_LL_O0_MAX_FN_BYTES` overrides it, keyed into the object cache
  alongside the other codegen env vars (#6394), so a project that *wants* the
  trade can take it (`PERRY_LL_O0_MAX_FN_BYTES=1048576`) without a rebuild and
  without imposing it on everyone.
- The oversized-unit diagnostic now prints the **widest** function next to the
  average. The average is precisely the statistic that cannot explain the
  routing decision, and without the widest figure the corpus measurement above
  is not possible.

Two tests bracket the cutoff with absolute sizes, and are verified discriminating
by sabotage: deleting the arm and raising the cap to 16 MiB each turn the
monolith test red; lowering the cap to 256 KB turns the `-Os` test red. An
earlier version that derived its fixtures from `ll_o0_max_fn_bytes()` stayed
green under the 256 KB sabotage — vacuous, and caught by running the sabotage
rather than reasoning about it.

Files: `crates/perry-codegen/src/linker.rs`,
`crates/perry-codegen/src/linker_tests.rs`,
`crates/perry/src/commands/compile/object_cache.rs` (+ its tests).
