Closes #7167: `perry compile --no-link` no longer leaks its object staging
directory into the system temp directory, and now writes its objects where
`-o` points.

`run_pipeline.rs` created a per-invocation `perry-objs-<pid>-<nanos>/`
directory for every compile and removed it on the paths that *link*.
`--no-link` returns before those, so it removed nothing. Because the name
carries the pid **and** a wall-clock nanosecond component, no two invocations
ever reuse one: the leak is unbounded in **compiles**, not in distinct IR the
way #7144's `.ll` leak was, and the staged objects are far larger than the
`.ll`s. The machine this was written on had accumulated 3086 such directories
(277 MB). Every `--no-link` user was affected — the flag itself, the
separate-link workflow, and every harness in
`scripts/compiler_output_harness/` (the census, knob-isolation and determinism
gates all compile with `--no-link`), which is why running the representation
census bled gigabytes a day.

**The objects could not simply be deleted.** On `--no-link` they are the
product: the flag is documented as "produce object file only", and the census
and knob-isolation gates hash the paths it prints (`_written_objects` raises
outright if a reported object does not exist on disk). What was wrong was
*where* they went, not that they survived. The flag also did not honour `-o`
at all — the census README carried a warning about it and a hand-written A/B
recipe built around the wart.

Two structural changes rather than a third `remove_dir`:

* **`--no-link` no longer creates a staging directory**, so it cannot leak
  one. Its objects are delivered to `-o`: verbatim when the program has one
  native module (`cc -c foo.c -o foo.o`), otherwise into `-o`'s directory
  under the module-derived names, because one `-o` cannot name N files. With
  no `-o` they land in the current directory. The rule keys on the module
  count — a property of the program — rather than on how many objects codegen
  actually wrote, so `-o` does not mean two different things depending on
  whether the object cache was warm. Bitcode-link mode emits `.ll`, never
  takes `-o` verbatim.
* **When linking, the staging directory is removed by `Drop`** (new
  `crates/perry/src/commands/compile/object_staging.rs`), so both link exits,
  the static-archive exit and every `?` in between clean up through one site.
  Three call sites that must each remember is how the third came to be
  missing. The default direction now matters: a future exit that does nothing
  cleans up, where before it leaked.

Removing the directory is unobservable to a concurrent compile, and that is a
property of the name rather than a timing argument: pid + monotonic nanos means
it belongs to exactly one invocation. This is the same conclusion #7144 reached
for the `.ll` by a different route — there the fix had to *create* per-call
ownership, because #7131 had made the `.ll` basename a pure function of the IR
and two workers holding identical IR shared it. Nothing is shared here.

Two further leaks the same guard closed, both broader than #7167 described:

* The **executable** link — the default path — never removed the directory at
  all. `cleanup_intermediates` only unlinks files, so every successful
  `perry compile` left an empty `perry-objs-*` directory behind. Empty, but one
  per compile.
* The static-archive and shared-library exits used `remove_dir`, which is
  non-recursive and silently no-ops when anything in the directory was not on
  the cleanup list. `Drop` uses `remove_dir_all`.

`--keep-intermediates` is still the single opt-in for retaining staged objects,
disarmed once where the directory is created rather than re-checked at each
exit. The codegen-failure paths now name the directory and say whether it
survives; on `--no-link` a failed compile keeps every object it managed to
write, at the path the user named.

**Gate.** `census-temp-hygiene` (#7144) shipped with a carve-out: it failed on
the clang driver's own temp names and merely *reported* anything else, because
this leak was live at the time and a gate that goes red for another module's
defect gets muted rather than fixed. The carve-out is gone — the gate now
asserts the absolute property, that an isolated `TMPDIR` is empty after the
corpus compiles, with **no allowlist**. #7167 is the argument for that: it was
known, printed on every run, and could not turn a run red for a full release.
The self-test asserts the flip directly (`perry-objs-*` inputs that returned 0
now return 1) and asserts that a name from neither family fails too.

**Evidence.** Two arms built sequentially from one target dir, distinct binary
hashes, isolated `TMPDIR`, 27 census workloads × 2 compiles = 54:

| arm | `perry-objs-*` entries left | harness exit |
|---|---|---|
| `main` | **108** (54 directories + 54 objects) | 1 |
| this branch | **0** | 0 |

The absolute property, not "no growth" — #7144's lesson, and here growth would
have caught it, but on the next content-addressed leak it would not.

No behavioural change: across all 27 census workloads the emitted objects are
**byte-identical** to `main`'s, and a linked two-module executable is
byte-identical and runs identically. That is structural rather than lucky —
`compile_ll_to_object` returns the object *bytes* and `run_pipeline` writes
them, so the staging path was never in the object. Verified directly: no
`perry-objs`/`perry_llvm` string and no `__debug_*` section appears in a Perry
Mach-O object, with or without `PERRY_DEBUG_SYMBOLS=1`.

`census-determinism --repeat 3 --jobs 4` is 27/27 byte-identical on Darwin
arm64 on both arms.
