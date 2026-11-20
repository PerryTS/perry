Add `scripts/registry_lifetime_check.py`, a `lint` gate that fails when a map-like
handle/registry static has no production removal path (#11511). The Web Fetch
handle registries were only ever `remove`d in tests, so a server leaked every
request and response until the id band ran out (#11164/#11165); the existing
root-holder census proves a table is *scanned*, not that it *shrinks*.

The gate reuses `gc_runtime_root_holders.py`'s file set and declaration parser
over `perry-runtime`, `perry-stdlib`, `perry-ffi` and every `perry-ext-*` crate,
blanks `tests/`, `*tests.rs` and `#[cfg(test)]` items, and requires each
`HashMap`/`BTreeMap`/`DashMap`/set/`Slab` static to be touched by a production
function that removes (`remove`/`retain`/`drain`/`clear`/…, or a whole-table
`take`/`mem::take` on a line naming it). Touching follows function-body
accessors and borrow/guard/closure accessors (`with_nodes(|m| m.remove(..))`),
scoped so a same-named static or helper in another module cannot vouch.

Everything else carries a verdict in `scripts/registry_lifetime_allowlist.json`
(`bounded_by_program`, `bounded_by_constant`, `bounded_by_cap`,
`process_lifetime`, `diagnostic_only`, `removed_elsewhere` with a checked
`site`, or `open_leak`). An unlisted registry fails, and so does a stale entry.
The initial inventory names 34 `open_leak` tables — among them ext-streams'
writable/transform streams, the container handle registries, `FILE_BLOBS`,
`WASM_IMPORT_OBJECTS`, the async_hooks handle sets and per-server `PENDING`
queues — as follow-ups. `--self-test` plants test-only and `#[cfg(test)]`-only
removal (must fail) and production removal through a sibling file, an accessor
and a closure helper (must pass). Sabotage-checked on the real tree: deleting
the `table.remove(id)` calls in `fetch/lifecycle.rs::release` turns all five
fetch registries red.
