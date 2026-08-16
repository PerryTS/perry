### Fixed — `lint` was red on `main`

`scripts/check_thread_locals.py` is wired into the `lint` job and **exits 1 on
clean `main`**, so the job has been failing for every commit since the two PRs
that introduced the violations landed earlier today (#7312 and #7382). Per
CLAUDE.md's own gate taxonomy this is the worst variety: a required context that
is red for a pre-existing reason trains everyone to bypass it, and the next
genuine failure arrives indistinguishable from the standing one.

Four raw `thread_local!` blocks, all added today:

* `crates/perry-runtime/src/dyn_eval/interp.rs` — **two** blocks against one
  recorded, and the top-level one is explicitly hot by its own comment:
  *"Nested function/arrow expressions evaluate many times (ajv validators run
  per request)"*. On Darwin each read is a `_tlv_get_addr` **call** (#7469), so
  this is the exact shape the policy exists to prevent.
* `crates/perry-runtime/src/module_require.rs` — `PENDING_REQUIRE_PARENT`, on
  the `require()` path.
* `crates/perry-runtime/src/node_vm.rs` — `VM_INTRINSIC_GLOBAL`, `VM_CONTEXTS`.
* `crates/perry-runtime/src/process/node_module/source_map.rs` —
  `SOURCE_MAP_PROTOTYPE`, `SOURCE_MAP_CACHE`.

All four converted to `crate::perry_thread_local!` rather than recorded as cold.
The macro takes both the `= const { … }` and `= expr` forms and keeps `.with()`
at every call site, so each is a one-line change; `cargo check -p perry-runtime`
is clean with zero warnings. Conversion is the right default here — the script's
own documentation says recording a declaration that is not cold "would be
recording the wrong fact … and would spend the allowlist's credibility on
entries no one can justify", and at least one of these is demonstrably hot.

Re-recording the allowlist also drops the now-stale `interp.rs` entry (the file
has no raw blocks left, and the checker fails on a stale entry as well as a new
one — an entry nobody has to justify any more).

**`_hot_declarations` moves 174 → 225, and 43 of that was already wrong.**
Recomputing the field on pristine `main` yields 217, not the committed 174, so
the counter had drifted through earlier PRs that converted declarations without
re-running `--update`. The field is informational — only `--self-test` asserts
it — so nothing was gated on the stale value, but the number in the file was not
the number in the tree. It now is.
