Regenerated `docs/src/api/reference.md` for the 15 dispatch-table entries this
change adds: **2855 → 2870**. `docs/api/perry.d.ts` is unchanged at 2026, which
is correct — the added rows are dispatch-table entries, not public API surface.

The PR added the entries without regenerating, so `lint`'s "Check for API docs
drift" step (`git diff --quiet -- docs/src/api/reference.md docs/api/perry.d.ts`)
failed on the assembled tree.

Regenerated from a built binary, not hand-edited, and checked for the failure
mode that gate has: `scripts/regen_api_docs.sh` hardcodes
`<worktree>/target/release/perry`, and when that binary is absent it
regenerates from nothing and leaves both files **truncated**. A real
regeneration moves the header counts and leaves the tail intact; truncation
cuts the end. Both tails were verified present afterwards, and `perry.d.ts`
staying at 2026 is the corroboration — a truncating run would have emptied it
too.
