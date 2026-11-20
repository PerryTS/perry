Runtime changes now select the `perry-stdlib` unit tests in per-PR CI, covering
the GC, arena, closure and event-loop interactions previously skipped by the
reverse-dependency selector (#11423). Extension shims remain outside dependency
fan-out unless directly changed. Required lint coverage checks the selector’s
runtime, direct-change, unrelated, metadata-only and full-workspace cases.
Library-target detection now recognizes explicit Cargo crate types such as
`rlib`, so the selected stdlib suite actually runs with `--lib --bins` instead
of being skipped by `--bins`.
