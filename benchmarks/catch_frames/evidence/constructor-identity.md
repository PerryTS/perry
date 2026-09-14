# Constructor marker regression witness

The catch-frame optimization changes Thin LTO layout. The first optimized
build broke `test_gap_6301_event_target_subclass`: the plain EventTarget worked,
but a subclass had no `addEventListener` or `dispatchEvent`.

Both baseline and unguarded candidate were built from main `eb13fa188d` with
the same three-package release graph and the instruction-measurement settings
in the parent README. The only subsequent production change for the guarded
build is the seven-line marker guard in `object/global_this/fetch_globals.rs`.

`nm -C` found three local copies of `global_this_builtin_noop_thunk` in the
unguarded debug fixture. At `js_register_class_parent_dynamic`, GDB observed
that the parent closure used the thunk at image offset `0x6e2b00`. The
constructor classifier compared with image offset `0x41de30`, which had been
merged with `collected_pipeline_error_noop`. Those unequal addresses explain
the missed builtin-constructor classification.

The two checked-in GDB transcripts use the same binary. `parent.gdb.log`
records the failure and exit 1. `parent-identity.gdb.log` records changing only
the closure function pointer to the classifier's comparison address: the
whole fixture then exits 0 with the expected subclass behavior. No exception
savepoint or transport state is changed by that debugger intervention.

The guard gives the marker one `#[no_mangle]` symbol, `#[inline(never)]`, and a
body containing `black_box` of its own address. The guarded linked fixture has
one global `T global_this_builtin_noop_thunk`. It passes these existing compiled
regressions, with byte-identical Node/Perry stdout:

- `test_gap_6301_event_target_subclass`
- `test_gap_6336_class_expr_builtin_parent`
- `test_gap_builtin_alias_construct_7524`
- `test_gap_dynamic_builtin_construct_dispatch`
- `test_gap_node_util_3098_3099_3334`

The unguarded optimized build fails all five. Clean baseline passes four;
`test_gap_6336_class_expr_builtin_parent` also fails on baseline. This makes
the unguarded build a real guard-omission witness, rather than an assertion
about the guard's spelling. Full baseline and guarded recheck reports are in
`constructor-identity/rechecks.json.gz`; the guarded full parity replay also
includes these fixtures. LTO is necessary to exercise this failure mode;
default Rust unit tests alone do not prove marker identity across archives.

To repeat the omission: remove only the added guard lines, rebuild the three
packages with the README's settings in an isolated target, freeze that build,
and run `run_parity_tests.sh --filter test_gap_6301_event_target_subclass` with
`PERRY_SKIP_BUILD=1`, `PERRY_BIN` and `PERRY_RUNTIME_DIR` pointing to it, and
`PERRY_WORKSPACE_ROOT` pointing to the matching source. Restore the guard,
rebuild with the same graph, and require the fixture to pass. This witness was
observed with the pinned toolchain; another LTO version may partition code
differently.

The original binaries, compile logs and unabridged stdout are retained under
`/root/js-throw-evidence/eventtarget-triage` on perrymaster. The unguarded
compiler/archive sets are in the verified `pre-marker-artifacts.tar.gz` beside
that directory. Final performance and integration artifacts include the guard.
