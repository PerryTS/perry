### CI: three broken gates repaired, three real defects filed (#7977, #7971, #7970)

Three gates were unable to do their job. None was fixed by weakening what it
asserts; where an arm was red because it had found a real defect, the defect was
filed and the arm left red.

**`windows-build` was red before it ran anything (#7977).**
`scripts/check_thread_locals.py` read Rust sources with a bare
`Path.read_text()`, which decodes with the host locale — cp1252 on a Windows
runner. Fifteen files under `crates/perry-runtime/src` carry a byte cp1252 cannot
map; `i18n.rs` is reached first at offset 31552 with 923 newlines before it, so
`core.autocrlf` puts the failure at position 32475, matching the traceback to the
byte. That is the job's *first* step, so the seven behind it — including the only
Windows run of the `perry-runtime` unit tests, the only Windows build of the
runtime, stdlib and both Windows UI crates, the Windows parity smoke, the
VERSIONINFO check and the COFF-trimming test — were `skipped` on every PR. #7882
fixed the path-separator half of this class in the same file and the encoding in
`gc_runtime_root_holders.py`; these four readers were missed. Every call site now
passes `encoding="utf-8"` explicitly, and a new
`scripts/check_locale_independent_io.py` AST-scans the six Python files the
Windows audit step runs and fails on locale-defaulted text I/O. It runs in `lint`
— Linux, per-PR, already required — so the class is caught before it can reach
Windows. Static rather than `PYTHONWARNDEFAULTENCODING` because that only fires
on calls that execute, and because `tests/test_gc_ratchet.py` embeds `open(...)`
inside probe source *literals* that an AST correctly ignores.

**`llvm-inprocess` reported green while skipping its only real job (#7971).**
On PRs it ran `changes=success, native-backend=skipped` and concluded `success` —
CLAUDE.md's fourth way a gate cannot fail, in its purest form. A new
`llvm-inprocess-complete` fan-in now states EXERCISED or NOT EXERCISED in the log
and step summary, and fails when the real job failed, was cancelled, or was
skipped on a non-PR event (every non-PR event sets `relevant=true`
unconditionally, so a skip there means the post-merge anchor stopped anchoring).
When the job did run on `main` it failed with a naked `exit 1`: the smoke step
used bare `grep -q`/`cmp` under `set -euo pipefail` with each compile's stderr
redirected to a file nothing ever printed. It now names the failing command with
its real exit status, dumps the captured output, and diffs on a parity failure.

**`gc-native-roots` had never had a green run on any branch (#7970).** Its three
failing arms had three unrelated causes. The `macos-14` arm was a *gate* defect:
it asserted evacuation liveness without setting `PERRY_GC_DIAG=1`, and
`[gc-copy-minor]` — the only input `gc_evacuation_liveness_assert.py` reads — is
printed only under that flag, so the arm reported "evacuated NOTHING" on every
run since it was written and could never pass. With the flag, the same binary
under the same GC env reports 75 copying minors and 16277 objects copied and the
whole step passes; the collector had been evacuating the whole time. That assert
now separates "the instrument was off" from "the subject was dead", and gained a
five-direction `--self-test` wired into `lint`. The Windows arm's second failure
was Git-bash GNU tar reading `$RUNNER_TEMP` (`D:\a\_temp`) as a remote
`host:path`; fixed with `--force-local` plus a post-extraction check.

**Defects found and filed rather than silenced:**

- **#7982** — `PERRY_LLVM_INPROCESS=native` cannot build RS4GC `ptr addrspace(1)`
  root slots; `dialect/mod.rs::basic_type` has no `addrspace(N)` arm, and it is
  not a one-line fix. The unit corpus gates stayed green throughout because all
  three tracked `.ll` corpora were frozen on 2026-08-03, 151 codegen commits ago,
  and contain zero `addrspace(1)` — so `corpus_spike ... ok` proved the tests ran,
  not that they test the IR the compiler emits today.
- **#7984** — on aarch64 Linux, `PERRY_STACKMAP_WALKER=verify` catches the fast
  fp-chain walker and the unwinder resolving the same root to addresses 96 bytes
  apart. The fast walker is the one that runs when verify is off.
- **#7985** — `perry.exe` cannot link against the official LLVM 22 Windows
  release: `/MT`-vs-`/MD` CRT mismatch, the bundled rpmalloc redefining
  `malloc`/`free`, and inkwell referencing target backends the release does not
  build. Probably latent in `windows-build` too, whose build step has not executed
  since #7977.

Nothing was promoted to a required context. `gc-native-roots` in particular must
not be, while #7984 and #7985 keep two arms legitimately red; a STATUS block at
the top of that workflow now records which arm is which so the next reader does
not re-derive it.
