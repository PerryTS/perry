**Fixed:** the temp directory no longer accumulates one LLVM IR file per distinct
module ever compiled (#7144).

`compile_ll_to_object` stopped unlinking its temp `.ll` in #7135, deliberately:
that change had just made the name a pure function of the IR (#7131 — clang
records a translation unit's basename into the ELF object), so two workers
holding identical IR **shared** the path and a per-call unlink could race a
sibling that had computed the path but not yet handed it to clang. Nothing else
deleted them, and because the name is content-addressed the leftovers are bounded
by *distinct IR ever compiled on the machine* — which grows without limit in
practice, since working on the compiler changes the IR on nearly every rebuild.
1627 files / 951.8 MB after a day on one box; 1069 files / 635 MB still sitting in
the temp dir of the machine this was fixed on.

The fix removes the sharing rather than making the deletion more careful. Every
`.ll` → `.o` compile now gets a directory it owns —
`perry_llvm_scratch_<pid>_<counter>/` — and the directory is removed once the
object bytes have been read:

* no two calls, in one process or across processes, are handed the same `.ll`
  path, so the unlink has nothing to race and there is no narrow window left to
  lose. That matters because a narrow window is not testable: sabotaged to the
  naive shape (one flat shared `.ll`, unlinked after use) the 8-way concurrent
  test added here went red in one full-suite run and green in the next three;
* the *basename* inside the directory is still `perry_llvm_<fnv1a64(ir)>.ll`, and
  the object records the basename and nothing else — so emission determinism
  (#7131) is untouched, and the `.o` keeps the pid + counter that #7140 restored;
* failures keep their IR, and the error message names it. `PERRY_LLVM_KEEP_IR`
  keeps everything, now collected in one directory (`.ll`, `.o`, `.clang-stderr`,
  compile plan) instead of scattered across the temp root.

**`PERRY_DEBUG_SYMBOLS` is not an exemption, and the reason it was believed to be
one was wrong.** `-g` was documented as pulling the `.ll`'s absolute path plus
`DW_AT_comp_dir` into DWARF, which would have made the file part of the shipped
object. Measured on a real Perry module (Apple clang 21, `-target
x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`): the `-g` object is
**byte-identical** to the one without it and has **no `.debug_*` sections at
all**. Perry's codegen emits no `DICompileUnit`/`DIFile`/`!dbg` metadata, and
`clang -g` on a `.ll` lowers debug info already in the IR rather than synthesising
a compile unit for the input file. The second temp-file layout that premise
justified was implemented and then deleted — a mode justified by something that
does not happen is an unexercised configuration, not a feature. Not measured on
COFF/Windows.

**New checks**, all sabotage-verified in both directions:

* `census-temp-hygiene` (`scripts/compiler_output_regression.py`) compiles the
  census corpus with `TMPDIR` pointed at an empty directory and asserts the
  directory is still empty. Red on the old compiler (27 leftovers from 54
  compiles), green on the new one; wired into the `repsel-census` CI job with a
  verdict self-test. Note the shape of that number — repeats share a content hash
  and so share a filename, which means a *"no growth run-over-run"* check would
  have been **green on the broken compiler**. That is why CI never saw this while
  developer machines filled up, and why the gate asserts the absolute property.
* `the_ll_directory_is_not_recorded_in_the_object_but_the_basename_is` compiles
  one `.ll` under the same basename from two different directories, for both Linux
  ELF targets, and asserts the objects are identical — with a live control that a
  *different* basename does change them. This is the property the whole design
  rests on; until now it lived in a comment and one hand measurement taken on a
  Raspberry Pi. It cross-compiles, because the embedding is a property of the ELF
  writer rather than of the host, so it also runs on macOS where this defect class
  is otherwise invisible.
* `debug_symbols_do_not_change_what_the_object_records` pins the measurement
  above, including a direct assertion that the `-g` object carries no `.debug_`
  section name.

Verification: 27/27 census workloads emit **byte-identical objects** before and
after (this renames files and nothing else); `census-determinism --repeat 3
--jobs 4` and `--repeat 4 --jobs 8` all green; 24 concurrent `perry` processes
compiling one identical source, three times, 0 failures and one object hash.

Filed separately as **#7167**: the compile driver leaks its
`perry-objs-<pid>-<nanos>/` object staging directory on the `--no-link` path
(`run_pipeline.rs` removes it on both *link* exits and there is no third one).
That one is unbounded in the number of compiles rather than in distinct IR. The
new gate reports it without failing on it — a gate that goes red for another
module's defect gets muted rather than fixed — and widening it once #7167 lands is
a one-line change.
