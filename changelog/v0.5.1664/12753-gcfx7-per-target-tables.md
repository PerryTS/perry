The GC call-effect table a compilation lowers under is now exactly the
`.gc-effects` sidecar of the runtime archive it links (GC effects design,
slice 7): per target and per variant (default or instrumented), no longer
joined with the maximum of the three checked-in tables. The checked-in
`crates/perry-codegen/src/gc_effects/*.tsv`, the Python analyzer
(`callgraph.py`, `self_test.py`, `why.py`), `regen.sh` and the drift CI jobs
are gone; the CI jobs now write the sidecars of freshly built Linux, macOS and
Windows archives, run the analyzer self-test and check the pinned runtime
facts over the sidecar (`perry-gc-effects` test `workspace_sidecar`). A
compilation no sidecar speaks for (a `--no-link` compile with no runtime
archive, Geisterhand's cargo-built runtime, the runtime copy inside the
Android/watchOS/visionOS UI library) compiles under the all-unsafe table.
On Linux, 21 runtime symbols that only another target could not prove are now
proven (class-registration helpers such as `js_register_class_id` become GC
leaves).
