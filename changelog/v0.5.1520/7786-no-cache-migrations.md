### Changed

**The update cache carries its shape, and a foreign shape is discarded rather
than migrated.** `~/.perry/update-check.json` now records a `schema` number; a
value this build does not recognize — including its absence, which reads as `0` —
means the file is thrown away and the next check rewrites it.

This replaces the alternative, which was to keep every field optional forever so
that older shapes still load. That trade is a bad one for a cache: it buys one
saved network request in exchange for a set of `Option` fields that only exist
to describe versions nobody runs, and that nothing ever removes. Bumping
`CACHE_SCHEMA` is now the whole migration story.

**One spelling per check source.** `github`, `npm-registry` and
`github-packages` are gone; the names are `gh-releases`, `npm`, `gh-registry`
and `custom`. A set of accepted aliases is a surface to document and test
forever in exchange for saving one look at the docs.

An unknown `source` still falls back to the default rather than failing, but for
a different reason than compatibility: an update check is the wrong place to
turn a config typo into a hard error.

<details>
<summary><b>Tests</b></summary>

The test that asserted a pre-throttle cache still loads is replaced by one
asserting the opposite — that a foreign schema, and an absent one, are both
recognized as not-ours. Verified at runtime as well: a planted cache with no
schema, claiming version `99.0.0` and a `last_check` in 2099, was ignored, a
real check ran, and the file came back stamped `"schema": 1`.

`cargo test -p perry`: 930 passed, 0 failed.
</details>
