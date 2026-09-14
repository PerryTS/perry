# Runtime handle scopes — 2026-09-14

Baseline: `9fda98df68d9fac3c08b2385fae007aa9f5278df` (Perry 0.5.1563).
Linux host: perrymaster.skelpo.net, x86_64. The baseline was built from a clean
worktree; the promise profile was recorded before runtime edits began.

## Instruction counts

Whole-program user instructions, `perf stat -e instructions:u -r 3`:

| workload | baseline | changed | reduction |
|---|---:|---:|---:|
| 100,000 promise/await iterations | 1,100,168,994 | 1,050,989,735 | 4.47% |
| 100,000 JSON round-trips | 3,190,619,152 | 3,132,631,719 | 1.82% |
| Original hoisted regex `test` probe | 13,773,038,016 | 13,003,278,589 | 5.59% |
| Original regex `exec1` probe | 31,188,918,430 | 29,854,497,805 | 4.28% |

Every binary's stdout matched Node 26.5.1. These are complete workload counts,
including startup and input construction, without subtracting a loop control.
No wall-clock speedup is claimed on the shared host. The measured build uses
the repository's release profile with 16 codegen units, debug level 1, and no
symbol stripping; both sides use identical settings and package selections.

`perf record -e instructions:u -c 1000003 --call-graph dwarf` produced the
folded stacks in `evidence/`. The pre-edit promise profile had 1,097 samples;
4.10% of leaves named runtime handles or operations on their slots. JSON had
3,189 samples and 3.98% such leaves. This classifier includes inlined primitive
slot loads/stores, not just function-call overhead. The corresponding changed
shares are 4.29% and 0.48%; residual handle work does not vanish in every probe.
Inclusive shares also contain callbacks invoked through handle helpers and
must not be interpreted as the cost of rooting itself. Instruction totals,
rather than these small sampled shares, are the performance verdict.

## Reproduction

Build each source tree independently with the same environment:

```sh
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
export CARGO_PROFILE_RELEASE_DEBUG=1
export CARGO_PROFILE_RELEASE_STRIP=none
export CARGO_INCREMENTAL=0
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
PERRY_RUNTIME_DIR="$PWD/target/release" PERRY_NO_AUTO_OPTIMIZE=1 \
  python3 benchmarks/runtime_handle_scopes/measure.py \
  --perry "$PWD/target/release/perry" --output /tmp/handle-profile
```

For the recorded A/B, `PERRY_BUILD_COMMIT` was pinned to the baseline SHA for
both complete builds and their compiler invocations. This intentionally permits
relinking the changed runtime while keeping coherence stamps equal; it does not
replace rebuilding both static wrappers. Compiler/archive hashes are retained
in `evidence/` and under `/root/runtime-handle-evidence/` on the measurement host.
The later GC stress build also selects `perry-ext-net` and `perry-ext-http`
to unify their Tokio configuration with the stdlib. Its runtime archive hash
is unchanged; its compiler and stdlib hashes differ from the measured build.
Raw perf data
and validation logs are also retained locally in
`/tmp/perry-runtime-handle-evidence/`.

## Contracts and tradeoffs

The live prefix is still `[0, top)`. Handles remain indices so buffer growth
does not invalidate them, and the scope lifetime and bounds/kind checks remain.
On native TLS targets, scopes and handles cache a reference to non-dropping
metadata; its `Cell` fields prevent sending them between threads. These scopes
and handles grow from one machine word to two. Android and HarmonyOS retain scoped Vec
access through a thread-bound token because its OS-backed TLS frees even
non-dropping metadata at teardown. Each root slot shrinks from 24 to 16 bytes
by encoding the raw tag in the variant discriminant. The merge train retains
the original four-slot initial capacity (64 bytes with the smaller slots), then
doubles it. The instruction measurements above used the PR's earlier 64-slot
initial capacity; they do not measure the changed initial growth schedule.

The existing named hot-TLS pointer and its offsets stay in place. The fallback
metadata can be accessed without filling the hot cache, preserving #9183's
initialization contract. On native TLS targets, a separate guard owns the buffer, unpublishes its
cache pointer, and clears the live prefix before freeing it. Late scope
destructors see empty metadata; a push after buffer destruction fails through
the destroyed owner guard instead of resurrecting storage. On OS-backed TLS, the Vec owns
its allocation and late scopes resolve the TLS key again instead of retaining
an address after teardown. Cache unpublication tolerates an Android pool that
has already been destroyed.

Scanners visit local slot copies and commit relocations by index. This avoids
references into reallocated storage, including a reentrant legacy Copy visitor;
a non-rewriting callback cannot have its own update overwritten. Throw restore
and scope drop both retain `Vec::truncate`'s non-growing semantics. The root
write barrier tests the existing global idle flag before decoding slot kind;
an active cycle still uses the existing per-thread shading machinery.

## Failure evidence

`fault-results.json` records all four injected faults:

| fault | observed failure |
|---|---|
| Scan `[0, top - 1)` | Moving-root test: final root 256 was not relocated. |
| Skip throw-savepoint truncation | Real caught-throw test: live depth does not return to its saved value. |
| Skip root shading during marking | Marking test: newly published handle target is still white. |
| Skip cache unpublication | Late TLS destructor asserts that the cache pointer is null; process aborts. |

All mutations were removed. The restored full runtime suite passed 3,802 tests,
with four ignored and zero failures. Regex-off checking passed. Runtime clippy
diagnostics match the clean baseline exactly, including its 12 existing
`approx_constant` errors in tests. The root-holder custody and raw-handle debt
ratchets pass without modifying their inventories.

The GC ratchet was measured with seven runs per probe on macOS arm64. Both
builds match Node on all 14 probes, and all 126 heap/cycle/copy/promotion/free
metric medians are identical. Both fail the same 30 cells against the older
pinned artifact; this is a reproduced baseline failure, not a green pinned
ratchet. The two compressed measurements, exact comparison, and shared list
of pinned failures are in `evidence/`. RSS and wall time are excluded from
this comparison, as required by the shared-host profile.

GC instrument smoke passes, including all 14 real probes and the forced
verification/evacuation checks. The full GC matrix matches Node in 595/602
cells (428 PASS, 167 UNVER); every required collection mode is live. The seven
failures are the HTTP/2 pending-event fixture in all arms: its server tries
port 443, already occupied on the host, and exits without callback output.
An independently rebuilt clean Linux baseline produces the identical empty
output and bind error; `http2-baseline-comparison.json` records both results.
In a private network namespace, both binaries time out after 20 seconds without
callbacks (`http2-netns-results.json`); this fixture remains unverified.
The matrix JSON is retained without suppressing those failures.

The full lint replay executes 83 gates: 80 pass, three fail, and two additional
CI-only checks are skipped. All three failures reproduce on the clean baseline:
public benchmark freshness, two dead-code errors in unchanged WebAssembly
helpers under the workspace warning check, and missing generated `bun-pty`
API documentation. The clean baseline compiler generates byte-identical API
documents to the changed compiler. The generated drift is retained as evidence
and is not included as an unrelated source change.
