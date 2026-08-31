### Fixed — gap snapshots now require a valid Node oracle

The nine regressions reported by a pristine-main gap run in #9273 were two
different artifacts. Five package-backed fixtures ran without the root
`npm ci`, so Node printed `ERR_MODULE_NOT_FOUND` while Perry produced valid
output. Four were genuine #9244 regressions that #9247 had already fixed, but
the stale run still presented them as current failures. The committed Linux
baseline remains the five failures reproduced by the pinned oracle.

`run_gap_tests.sh` now stops before building unless `node --version` exactly
matches `.node-version` and `npm ls --depth=0` confirms the committed root
dependency graph is present. Node is the byte-for-byte reference output, so a
different patch release or missing package is a different experiment rather
than evidence of a Perry regression. Both failure paths explain how to restore
the required environment.

Snapshot refreshes now copy existing issue/date/category/reason metadata from
`known_failures.json`, and the required offline audit checks the relationship
in both directions: every accepted Linux snapshot failure must have a
Linux-applicable, issue-backed known-failure entry. The formerly anonymous
