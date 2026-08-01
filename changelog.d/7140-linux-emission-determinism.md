**codegen:** the LLVM temp *object* path is per-process unique again, and
emission determinism is now a gate on every host instead of a Linux caveat
(#7131).

### The regression #7135 left behind

#7135 fixed #7131 by content-addressing the temp `.ll` name. It content-addressed
the temp `.o` name too, and that half was wrong: the `.o` used to carry the pid,
and lost it.

`TEMP_NONCE_COUNTER` is per-process state and every process starts it at `0`, so
two `perry` processes compiling identical IR both chose
`perry_llvm_<hash>_0.o`. `compile_ll_to_object` deletes the object once it has
read it — so they deleted it out from under each other. Measured at `a3b31c0d8`,
four concurrent compiles of one census fixture, three rounds: **8 of 12 failed**

```
Failed to read clang output at /tmp/perry_llvm_eee31bbdd9dc24a5_0.o:
No such file or directory (os error 2)
```

This is #509 again, one scope out: the pid that used to prevent it was removed
as collateral. It bites any parallel build of the same input — including every
A/B harness in `scripts/compiler_output_harness/`.

Both platforms, four concurrent compiles of one census fixture, before → after:

| host | `a3b31c0d8` | this branch |
|---|---|---|
| macOS `arm64` | **8 / 12 failed** | 0 / 12 |
| Raspberry Pi 5, aarch64 Linux | **3 / 16 failed** | 0 / 16 |

It is a race, so the rate is timing-dependent — the slower host loses fewer. It
is also why this was found by running the determinism repeats *concurrently*
rather than in sequence: a serial check never opens the window.

The atomic-write staging file had the same defect (two processes reach
`…​.ll.tmp.0` with the same hash and the same counter, and `File::create`
truncates), and is fixed the same way.

### Which names actually reach the object

The two names are asymmetric on purpose, and both halves have now been got wrong
once. Measured rather than assumed — aarch64 Debian clang 19.1.7, ELF, no `-g`:

| name | recorded in the `.o`? |
|---|---|
| `.ll` **basename** | **yes** — `STT_FILE` in `.symtab` |
| `.ll` directory, process CWD | no (needs DWARF, i.e. `-g`) |
| `-o` output path | no |
| `ld -r` input / output paths | no |

So the `.ll` must be a pure function of the IR, and uniquifiers are both free
and mandatory on every *output* name. That table is now a comment on
`TEMP_NONCE_COUNTER` so the next temp-path change can be reviewed without
re-deriving it.

Residual, stated plainly: under `PERRY_DEBUG_SYMBOLS` clang emits DWARF, which
pulls the absolute `.ll` path and `DW_AT_comp_dir` into the object. Those builds
are reproducible only for a fixed `TMPDIR` and working directory. Nothing else
in the emission path is known to vary.

### The gate

`census-knob-isolation` detected a nondeterministic host and **skipped** its
emission half. That workaround existed because of #7131 — so the half of the gate
that caught the `PERRY_CANONICAL_STR_LOCALS` leak could not run on Linux at all,
the host where object-hash A/B is most useful (it is the one with an unprivileged
instruction-retired counter). The skip and its `--require-emission` escape hatch
are gone: a determinism disagreement is a hard failure, on every host, before any
knob is judged.

- **`census-determinism`** (new) — compile the census corpus N times and compare
  the bytes; the standalone instrument for establishing that object-hash evidence
  is valid on the host you are measuring on. Repeats run concurrently on purpose:
  identical IR now shares one content-addressed `.ll`, so racing it is part of the
  subject — and that is what caught the collision above.
- Runs on the `ubuntu-latest` `repsel-census` CI job, the ELF host that can
  actually observe a relapse, alongside both verdict self-tests.

### Verified

Red-then-green on a Raspberry Pi 5 (aarch64 Linux) across all 26 census
workloads. Three arms built sequentially from one target dir — same toolchain by
construction — with three distinct binary hashes:

| arm | `census-determinism` |
|---|---|
| `22367565f` (before #7135) | exit 1 — **26/26 nondeterministic** |
| `a3b31c0d8` (#7135 as merged) | exit 0 — 26/26 identical |
| this branch | exit 0 — 26/26 identical (also at `--repeat 3 --jobs 3`) |

`suite_01_startup`, compiled twice serially, with the mechanism visible:

```
pre : objects DIFFER in 12 bytes
      STT_FILE  perry_llvm_217502_1785528949373123236_0.ll
      STT_FILE  perry_llvm_217533_1785528951945773193_0.ll
post: objects IDENTICAL
      STT_FILE  perry_llvm_2791e842224ea99c.ll   (both runs)
```

macOS (`Darwin arm64`): 26/26 workloads × 3 compiles → one hash each, so the
#7039 closure-iteration-order fix has not regressed. Mach-O does not record the
`.ll` basename at all (verified directly), which is why the original defect was
invisible there.

**No behavioural change**, by the strongest available measure: across all 26
workloads the objects emitted by `a3b31c0d8` and by this branch are
**byte-identical, 0/26 differences**. This change renames temp files and nothing
else, and the emitted bytes say so. The GC x representation-selection matrix
against the pinned Node 26.5.1 oracle agrees —
`gc_repsel_matrix.sh --arms all --pressure 8` on the Pi:
`PASS=426 UNVER=119 XFAIL=1 FAIL=0` over 546 rows.

`cargo fmt`: `linker.rs` was left unformatted by #7135, so `lint` is red on
`main` independently of this change; formatting it is included here.
