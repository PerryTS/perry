# Native export collision audit

`scripts/native_export_collisions.py` inventories unmangled function/static
exports in `perry-runtime`, `perry-stdlib`, `perry-ffi` and every remaining
`perry-ext-*` crate. It scans production source across features and targets,
including name-argument export macros and `export_name` aliases. It removes
items whose cfg requires `test`, follows conventional test-module declarations,
and ignores comments, string contents and imported extern declarations.

This is a source inventory, not a claim that all feature combinations link
these providers together. It excludes UI platform crates and detects collisions
across crates; it does not resolve cfg combinations or macro names assembled
with procedural macros/token concatenation. Unsupported name-argument export
macro shapes fail explicitly. Built archives remain the authority for the
symbols of one actual configuration.

## Provider decisions at main 9e29f59d43

The pg and mysql2 copies named by #10678 were already deleted, along with other
retired npm bindings. There were eight stale net exports in the orphaned
`perry-runtime/src/net.rs`; its module declaration was commented out and no
in-tree code referenced it. This change deletes that file and its obsolete
comment. The compiled provider is `perry-ext-net`.

The remaining 215 potential cross-crate collisions have live feature-gated
providers. Keep those providers until their routes can be consolidated with
behavioral coverage; deleting one in an export audit would break supported
bundled builds. The inventory records every exact symbol and source path in
`scripts/native_export_collisions.json`:

| Providers | Symbols | Decision |
|---|---:|---|
| stdlib / ext-zlib | 34 | Ext binding serves the well-known route; stdlib compression features still serve bundled builds. |
| stdlib / ext-events | 33 | Preserve `bundled-events` alternate provider. |
| stdlib / ext-streams | 33 | Preserve `bundled-streams`; Web Fetch also requires this surface. |
| runtime stubs / stdlib | 22 | Runtime-only fallback under `not(feature = "stdlib")`; preserve the real stdlib provider. |
| runtime stubs / ext-ws | 21 | Runtime-only fallback; `external-ws-symbols` suppresses it for ext links. |
| stdlib / ext-cheerio | 18 | Preserve `bundled-cheerio` alternate provider. |
| stdlib / ext-sharp | 18 | Preserve `bundled-sharp` alternate provider. |
| stdlib sqlite / ext-better-sqlite3 | 15 | Preserve `database-sqlite`; optimizer intentionally retains it for shared database APIs. |
| stdlib / ext-ethers | 8 | Preserve `bundled-ethers` alternate provider. |
| stdlib / ext-argon2 | 5 | Preserve `bundled-argon2` alternate provider. |
| stdlib / ext-bcrypt | 5 | Preserve `bundled-bcrypt` alternate provider. |
| stdlib / ext-nodemailer | 3 | Preserve `bundled-nodemailer`; shared SMTP engine remains separate. |

The well-known provider selection and bundled-feature stripping live in
`crates/perry/src/commands/compile/optimized_libs/driver.rs`. This audit changes
none of that selection, feature unification, code generation or runtime storage.

## Gate behavior

```sh
python3 scripts/native_export_collisions.py --self-test
python3 scripts/native_export_collisions.py
python3 scripts/native_export_collisions.py --no-raise-vs origin/main
python3 scripts/native_export_collisions.py --inventory
```

A new collision, third provider or moved source path fails with symbol and
locations. A removed collision leaves a stale record and fails until that
record is pruned. The PR-base check prevents recording a new collision in the
same PR to evade the gate. On its first introduction it derives the previous
inventory directly from the base commit's sources; subsequent runs compare
against the base's committed inventory. Both checks run in required `lint`
through `pr-gate` with `!cancelled()`, and synthetic controls prove that literal,
renamed and macro-generated collisions fail while clean/removal controls pass.

No runtime sidetable, cache, allocation or new dependency is introduced.
