### CI: unblock `security-audit`, a REQUIRED context that was red on every merge

`security-audit` is in branch protection's required set and had failed its last
six completed `main` runs, which means **every merge was bypassing it** — the
"REQUIRED + red ⇒ universal bypass" pattern CLAUDE.md names.

The sole unignored finding was **RUSTSEC-2026-0235** (`rkyv` 0.7.46, OOB reads
when validating archives containing `Rc`/`Arc`). It is **not in Perry's build
graph**: `rkyv` is an optional feature of `rust_decimal`
(`rkyv = ["dep:rkyv"]`) and `perry-stdlib` enables only `maths`. Verified three
ways — `cargo tree -e normal -i rkyv` reports "did not match any packages",
`cargo tree -p rust_decimal --depth 1 -e features` resolves to
arrayvec/serde/num-traits only, and there are zero `rkyv` artifacts in the dev
or release deps directories. `cargo audit` reads `Cargo.lock`, which lists
optional dependencies regardless of feature activation, so this is a
lockfile-only finding.

Added to the existing `--ignore` list with that evidence recorded inline, plus
a re-evaluation trigger (anything enabling `rkyv`/`rkyv-safe` on
`rust_decimal`). `cargo audit` now exits 0.

Also corrects the note on **RUSTSEC-2026-0187** (`lopdf`): dependabot #7412
proposes printpdf 0.10.1, which **still pins lopdf 0.39.0**, so that PR does
not close the advisory despite looking like it would.
