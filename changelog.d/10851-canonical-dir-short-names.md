`canonicalize_module_path`'s directory resolver rebuilt paths from **literal
component names**, so a Windows 8.3 short name survived and two spellings of
one module produced two registry keys (#10851).

`canonical_dir` resolved the parent (memoized) and then joined this component,
taking the join verbatim whenever the component was not a symlink. On Unix that
is exact — a canonical parent joined to a non-symlink component is canonical.
On Windows it is not: `C:\Users\RUNNER~1\...` stays short, while the *same file*
reached through a `..` component takes `canonicalize` and comes back long
(`runneradmin`).

`PathModuleRegistry` is keyed on that string, so `require("./m.js")` and
`require("./nested/../m.js")` registered as **two different modules, each with
its own initializer** — a CJS module must be a singleton. That is what
`canonical_alias_cannot_replace_the_first_initializer` has been catching on
`windows-build`, which failed on every `main` run sampled on 2026-09-21 at that
one assertion out of 4038.

Now one `std::fs::canonicalize` per NEW directory, still memoized — the same
budget the join form had (it spent a `read_link` per new directory) and the same
answer on Unix, while resolving short names, casing and symlinks in one step on
Windows. Falls back to the literal path when the directory does not exist,
as before.

Only reproduces where a component has a distinct 8.3 short name — a name longer
than 8 characters on a volume with 8.3 generation enabled. `runneradmin`
qualifies; a dev profile like `C:\Users\user` does not, which is why it was
stable on CI and invisible everywhere else.

Verified on macOS: `module_require` 23 tests and `path_module_registry` 14 pass.
The Windows path itself is unverified here — this host cannot build that target,
and the fix is written to be platform-independent rather than `cfg`-gated
precisely so it is type-checked everywhere.
