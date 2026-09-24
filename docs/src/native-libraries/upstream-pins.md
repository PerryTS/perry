# Well-known binding upstream pins

Every third-party npm package that ships a bundled native wrapper in
`crates/perry/well_known_bindings.toml` carries a **provenance pin**: the exact
upstream release the wrapper ports, its tarball content hash, and the date the
wrapper was last reviewed against it. This is the same discipline the
socket-registry fleet applies to its vendored upstream references, adapted for
npm dists.

```toml
[bindings.bcrypt]
crate = "perry-ext-bcrypt"
lib = "perry_ext_bcrypt"
tracking = "#466"

[bindings.bcrypt.upstream]
version   = "6.0.0"           # pinned npm release (immutable dist)
sha256    = "3b2478cd…"       # sha256 of the registry tarball at pin time
repo      = "https://github.com/kelektiv/node.bcrypt.js"
ported-at = "6.0.0"           # release the wrapper was last REVIEWED against
date      = "2026-07-30"
```

## The lock-step rule

**`ported-at` must equal `version`.** Re-pinning a binding to a newer upstream
release without re-reviewing the wrapper against the upstream diff reds the
`binding_pins.mjs --check` gate — and the perry binary itself refuses to load a
skewed table. An upstream release can never go silently stale, and a pin bump
can never outrun the review it demands: bumping `version` forces you to advance
`ported-at`, which forces the review.

## Exempt entries

Three kinds of row carry no pin:

- **Node builtins** (`node-builtin = true`): `zlib`, `events`, `net`, `http`,
  `https`, `http2`, `streams`. Their upstream is Node core, not an npm dist.
- **Aliases** (`alias-of = "<binding>"`): a package subpath (`mysql2/promise`)
  or a bare-name alias (`fetch` → `node-fetch`) that shares its target's
  provenance.
- **Perry-owned packages** (`@perryts/*`, `perry/*`).

Note that a distinct npm package served by a shared wrapper crate is **not** an
alias — separately published and versioned packages each carry their own pin,
even when they share a wrapper crate.

## Tooling — `scripts/binding_pins.mjs`

```sh
# Provision or bump one pin to a specific version (default: latest stable)
node scripts/binding_pins.mjs --set bcrypt 6.0.0

# Provision every currently-unpinned binding at its latest stable
node scripts/binding_pins.mjs --backfill

# Offline gate (CI): pins present, lock-stepped, crates exist. Exit 1 on any
# violation. No network.
node scripts/binding_pins.mjs --check

# Advisory: additionally flag pins whose upstream has a newer stable release
# that has soaked >= N days (default 7). Network. Run in the weekly update.
node scripts/binding_pins.mjs --check --refresh --soak-days 7

# Materialize the upstream repo at the pinned ref into gitignored upstream/<name>
# for port review (diff the old pin against a candidate new tag)
node scripts/binding_pins.mjs --materialize bcrypt
```

Never hand-edit `version` / `sha256` / `ref` — the tarball hash can't be
recomputed at edit time. Use `--set`, which fetches the registry tarball,
hashes it, records the publisher's `gitHead`, and stamps `ported-at`/`date`.

## Cadence

The `--check` gate runs on every PR (offline). The weekly dependency-update
job runs `--check --refresh`, so a newly-soaked upstream release surfaces as an
actionable advisory — re-pin with `--set`, review the wrapper against the diff
(`--materialize` helps), and land the bump with `ported-at` advanced. This
mirrors the fleet's `vendor-actions.mts --check` weekly cadence.
