# Upstream action references

`pins.json` records the upstream repositories, selected releases, and immutable
commit SHAs that were reviewed when Perry's workflow implementations were
written. Workflows do not call into this directory.

`pnpm run actions:check` compares each recorded SHA with its selected tag and
checks that Perry's corresponding implementation records the same source pin.
`pnpm run actions:update` resolves those tags and refreshes only these sparse
reference snapshots. It never overwrites Perry's implementation. The umbrella
`pnpm run update` runs this alongside the npm and Cargo dependency updater.

Perry-owned implementations live under `.github/actions/perry/`, and simpler
operations such as repository checkout run as inline workflow steps. When a
tag advances, review the refreshed snapshot and intentionally adapt Perry's
implementation; changing this reference must never silently change workflow
behavior.
