# CI gate repair: #7977, #7971, #7970

Working notes. Written incrementally; the PR body is the summary.

---

## #7977 — `windows-build` red before it runs anything  [FIXED]

### What this gate covers (and what was therefore unprotected)

`windows-build` is the **only Windows execution of anything** in per-PR CI. Its
step list, in order:

1. GC structural audits (Windows)  <- died here, 27 s in
2. Install Rust toolchain / LLVM 22 / Node
3. Build compiler + runtime + `perry-ui-windows` + `perry-ui-windows-winui`
4. **`perry-runtime` unit tests** (`RUST_TEST_THREADS=1`, #7356)  <- the big one
5. Windows parity harness smoke
6. `perry.exe` VERSIONINFO resource check
7. COFF duplicate-symbol archive trimming test

Steps 2-7 were `skipped` on **every** PR whose Windows run executed the current
`test.yml`. So for the duration: **no Windows compile of the runtime, the stdlib
or either Windows UI crate; no Windows run of the `perry-runtime` unit tests; no
Windows parity smoke; no VERSIONINFO check; no COFF-trimming test.** A Windows-only
miscompile, a Windows-only test failure, or a broken `perry-ui-windows` build would
all have merged green-by-skipping.

### Root cause (confirmed to the byte)

`scripts/check_thread_locals.py` reads Rust sources with a bare
`Path.read_text()`, which decodes with `locale.getencoding()` — **cp1252** on a
GitHub Windows runner. 15 files under `crates/perry-runtime/src` carry a byte
cp1252 has no mapping for (0x81/0x8d/0x8f/0x90/0x9d). Reproduced locally:

```
15 files undecodable as cp1252
   crates/perry-runtime/src/i18n.rs                 offset 31552  0x8d
   crates/perry-runtime/src/intl/duration_format.rs offset 17349  0x81
   ...
i18n.rs: offset=31552 newlines_before=923 crlf_position=32475   (issue says 32475)
```

The issue's arithmetic is exact: `core.autocrlf` widens 923 `\n` to `\r\n`, so
31552 + 923 = **32475**, the position in the traceback.

`#7882` ("make GC structural audits portable") fixed the *path-separator* half of
this class in this same file and the *encoding* in `gc_runtime_root_holders.py`,
but missed these four readers. Same shape, one file over.

### Changed

- `scripts/check_thread_locals.py` — all 5 reads and 15 writes now go through
  `read_source()` / `write_source()` helpers that pass `encoding="utf-8"`
  (and `newline=""` on write, so `--update` is byte-stable across hosts).
  The gated content is **unchanged**: the `files` map the checker verifies is
  identical before and after (asserted, see validation).
- `scripts/gc_runtime_root_holders.py` — one **latent** instance of the same
  class in the self-test's fixture writer. ASCII today, so it has not bitten;
  fixed with the rest.
- `tests/test_gc_ratchet.py:1369` — bare `read_text()` on the shipped baseline.
- `scripts/check_locale_independent_io.py` — **new gate** (below).
- `.github/workflows/test.yml` — new `lint` step running that gate;
  `PYTHONUTF8: "1"` on the Windows audit step as belt-and-braces.

### The new gate, and why it is static

`scripts/check_locale_independent_io.py` AST-scans exactly the six Python files
`windows-build`'s audit step executes, and fails on `open()` / `read_text()` /
`write_text()` / `Path.open()` without an explicit `encoding=` (binary modes
exempt).

Static, not `PYTHONWARNDEFAULTENCODING`, for two reasons:

- `EncodingWarning` only fires on a call that **executes**. A bare `read_text()`
  on an error path or in a subcommand CI does not invoke warns nobody and ships.
- `tests/test_gc_ratchet.py` writes Python probes as *string literals* containing
  `open(...)`. Those are not calls; the AST correctly ignores them, a grep would not.

It runs in **`lint`** — Linux, per-PR, and a **required** context — because the
defect is invisible on Linux at runtime. That is the point: the class is now
caught before it can reach Windows, rather than by taking the Windows job down.

### Validation

| check | result |
|---|---|
| reproduce main's failure under a non-UTF-8 locale | `UnicodeDecodeError: 'ascii' codec can't decode byte 0xe2`, exit **1** |
| same command, fixed | exit **0**, `thread-local policy OK: 173 hot declarations, 129 raw blocks in 91 recorded cold files` |
| full 8-command Windows audit sequence, non-UTF-8 locale | all **PASS** |
| `--update` output vs shipped allowlist | `files` map **identical** |
| **sabotage**: replant the exact #7977 defect | new gate exits **1** and names `check_thread_locals.py:115` |
| remove the replant | new gate exits **0** |
| new gate `--self-test` | OK — 6 flagged shapes, accepted shapes clean, scope asserted |

The sabotage arm is the one that matters: a green run of this gate means the
detector works, not that nothing was tried.

### Not changed (deliberate)

- `_hot_declarations` in `thread_local_cold_allowlist.json` reads 163 against an
  actual 173. **Pre-existing drift on `main`, not caused by this change**, and
  `verify()` does not gate on that field — only on `files`. Left alone to keep
  the diff reviewable; worth a one-line `--update` in a separate change.
- `gc_runtime_root_holders.py` raises `UnicodeEncodeError` when *printing* an
  em-dash under an ASCII locale. Measured: its output **is** cp1252-encodable, so
  this is an artifact of the stricter ASCII simulation and **not** a Windows
  defect. `PYTHONUTF8=1` on the step covers it regardless.

### Promotion

Nothing promoted. `lint` is **already** a required context; this adds a step to it.
That is deliberate — CLAUDE.md hazard 2 is the step people forget, so the gate is
placed where that step does not exist. It is green locally on this tree.

---

## #7971 — `llvm-inprocess` green-by-skipping on PRs, red when it runs  [GATE FIXED; defect filed as #7982]

### What this gate covers (and what was unprotected)

It is the CI exercise for `PERRY_LLVM_INPROCESS` — the GC knob kill-policy's
required arm for that mode — and #7966 calls it "the promotion prerequisite" for
the in-process LLVM backend. It covers: building with the feature, 528 unit gates
incl. the `.ll` corpus construction tests and the RS4GC pin, and an end-to-end
smoke over textual / `native` / `diff` modes plus a multi-unit split and the
#7302 exception-handling program.

Unprotected: on PRs, **everything** — `native-backend` was `skipped` and the
workflow still reported `success`. On `main` the job ran and failed, so its three
most recent executions asserted nothing either. Net: the in-process backend has
had no effective CI coverage, and `PERRY_LLVM_INPROCESS=native` is in fact broken.

### Root causes (three, independent)

1. **Vacuous green.** `changes` is a path filter; when it says "not relevant" the
   real job is skipped and the workflow concludes `success`. Sampled PR runs
   31505530279 / 31499833415 / 31476724152 are all `changes=success,
   native-backend=skipped`. A cost control was standing in for a verdict.
2. **No diagnostic.** `Native-mode smoke` ran bare `grep -q` / `cmp` under
   `set -euo pipefail` with each compile's stderr redirected to a file that was
   never printed. The three 2026-08-11 `main` failures therefore ended at
   `Generating code...` + `exit 1`.
3. **Frozen corpora.** The unit gate asserts `corpus_spike ... ok` to prove the
   corpus tests *ran*, but nothing asserts they are *current*.

### Changed

- New `llvm-inprocess-complete` fan-in (`if: always()`): prints EXERCISED /
  NOT EXERCISED to the log and step summary. Fails when `native-backend` failed
  or was cancelled, **and** when it was `skipped` on a non-PR event (every non-PR
  event sets `relevant=true` unconditionally, so a skip there means the
  post-merge anchor stopped anchoring — the #7856 shape).
- `Native-mode smoke` rebuilt around `run` / `assert_grep` / `assert_same`: names
  the failing command with its real exit status, dumps captured stdout+stderr,
  prints a diff on parity failure, and states what each liveness assert protects.
  Status is captured *after* the command — inside an `if ! cmd` branch `$?` is
  the negated status and is always 0.
- New "Corpus currency" diagnostic step printing corpus age and the count of
  IR-affecting commits since.

### The defect it was correctly reporting → #7982

Reproduced locally (LLVM 22 + `--features perry/llvm-inprocess`, 3m17s build):

```
native IR construction failed in @perry_closure_spike_ts__8:
in line: %r5 = alloca ptr addrspace(1)
```

`dialect/mod.rs::basic_type` handles `"ptr"` but has no `ptr addrspace(N)` arm.
Today's codegen emits **five** such shapes (alloca / load / store / inttoptr /
ptrtoint; 116 occurrences in `spike.ts` alone) for RS4GC root slots.

**Not a one-liner**: I added an `addrspace(N)` arm and rebuilt — `alloca` then
succeeds and the reader fails on `store ptr addrspace(1) null, ptr %r5`. Probe
reverted; filed as #7982 rather than folded into a CI change.

**Why the unit gate never saw it**: all three corpora were frozen 2026-08-03
(#7307/#7310), **151 codegen commits ago**, and contain **zero** `addrspace(1)`.
The gate asserting "the corpus tests ran" is green; its subject — the IR this
commit emits — never ran. Hazard 4 inside the liveness assert written to prevent it.

### Not done

- The `changes` filter does not include `crates/perry-hir` or
  `crates/perry-transform`, which also determine emitted IR. Widening it costs a
  90-minute macOS job on many more PRs, so it is a policy call, not a fix I made.
  Noted here rather than changed.
- Nothing promoted to required. It cannot be promoted while #7982 is open.

---

## #7970 — `gc-native-roots` has never been green  [1 arm fixed, 2 arms = real defects, filed]

### What this gate covers (and what was unprotected)

Native-frame GC roots (RS4GC) across every host shape Perry supports: aarch64
Mach-O, x86-64 ELF, x86-64 PE/COFF, aarch64 ELF. Per arm it asserts the compact
root map section exists, that `__llvm_stackmaps` did NOT survive, byte-parity
against the pinned Node oracle, walker-trace location telemetry, and evacuation
liveness — plus, on Windows, that the funclet-EH refusal stays a refusal.

Unprotected the whole time: **everything except the x86-64 ELF arm**. No green
evidence has ever existed for native roots on aarch64 Mach-O, aarch64 ELF or PE.

### Arm 1 — `macos-14` zero-evacuation: GATE DEFECT, FIXED

The in-process step ran the forced-evacuation binary **without
`PERRY_GC_DIAG=1`**, and `[gc-copy-minor]` is emitted only under that flag. It is
the only input `gc_evacuation_liveness_assert.py` reads, so the assert saw an
empty trace and reported "evacuated NOTHING (0 copying minors, 0 objects
copied)". The arm could never pass.

Reproduced and fixed locally on macOS aarch64, byte-for-byte including the
literal `<probe>` in the CI message (the step also omitted `--probe`):

| run | `[gc-copy-minor]` lines | assert |
|---|---|---|
| workflow env as-was (no `PERRY_GC_DIAG`) | 0 (stderr was 98 bytes of `#gcmetric`) | `evacuated NOTHING` — exit 1 |
| `+ PERRY_GC_DIAG=1` | 150 | `evacuation live — 75 copying minor(s), 16277 objects copied` — exit 0 |

Full step re-run locally with the fix: `__perry_gcmap` present, `__llvm_stackmaps`
absent, control-vs-forced stdout identical, assert green.

**#7970 hypothesised this might be #7965-shaped ("the collector stopped running
copying minors"). It is not.** The collector was evacuating the whole time; the
gate was asserting on telemetry it had not switched on.

To stop the misdiagnosis recurring, `gc_evacuation_liveness_assert.py` now
separates *instrument off* from *subject dead*: a trace with no `[gc-...]` marker
at all is reported as "`PERRY_GC_DIAG=1` was NOT set … fix the RUN, not the
collector", never as a statement about the GC. It gained a `--self-test` covering
5 directions, wired into `lint`.

### Arm 2 — `ubuntu-24.04-arm`: REAL DEFECT → #7984

Not a SIGSEGV (the issue read `Aborted (core dumped)` as one) — a Rust assertion:

```
stack_maps.rs:507: PERRY_STACKMAP_WALKER=verify: fast walk visited 1 unique slots, unwinder visited 1
  left:  [281474742909688]
  right: [281474742909592]      # 96 bytes apart
```

Both walkers found one slot and disagree on **where** it is. `fp_chain` is the
walker that runs when `verify` is off, so on aarch64 Linux the collector may be
marking and rewriting the wrong stack word — the CLAUDE.md "root-store dominance"
class, invisible at collection time. **This arm must stay red until #7984 is
fixed.** It is the gate working.

### Arm 3 — `windows-latest`: TWO problems

- **Real defect → #7985.** `link.exe` exit 1120: `/MT`-vs-`/MD` CRT mismatch
  (llvm-sys vs mimalloc), `LNK2005` on malloc/free/calloc/realloc from the
  rpmalloc LLVM's release bundles, and `LNK2019` for Mips/Lanai/Sparc/MSP430/
  XCore init symbols inkwell references. Filed, not fixed — it needs a
  Windows-toolchain decision.
  **Likely also latent in `test.yml`'s `windows-build`**, whose build step has not
  executed on any recent PR because of #7977. Expect it to surface once #7977 lands.
- **Workflow bug, FIXED here.** The second failure (exit 2) was Git-bash GNU tar
  reading `$RUNNER_TEMP` (`D:\a\_temp`) as a remote `host:path`:
  `tar (child): Cannot connect to D: resolve failed`. Fixed with
  `tar --force-local` plus a post-extraction existence check, so a bad archive
  reports itself instead of surfacing later as "no matched opt+clang pair".

### Promotion

**Nothing promoted, and gc-native-roots must NOT be promoted yet** — two arms are
legitimately red on filed defects, so making it required would block every PR. A
STATUS block at the top of the workflow records which arm is which, so the next
reader does not have to re-derive it.
