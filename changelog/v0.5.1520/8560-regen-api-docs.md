**Regenerate the API docs — `check` is red on `main` because they drifted.**

`check` fails on `main` at its "Check for API docs drift" step:

```
API docs drift detected. The compile-time manifest in
crates/perry-api-manifest/src/entries.rs changed but the
generated artifacts under docs/ weren't regenerated.
```

`crates/perry-api-manifest/src/entries.rs` gained the `@parcel/watcher` compatibility facade (#8532) plus entries from #8535 and #8525 without `./scripts/regen_api_docs.sh` being rerun. This is that script's output only, no hand edits: coverage moves from 2033 entries across 124 modules to 2051 across 134.

Verified: `check` goes from **fail to pass** with this change (it is red on `main` and on this PR's first push, green after).
