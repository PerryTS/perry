**The addr-class audit now scans `crates/perry-ext-*`** (#7272), bringing 18 previously-invisible sites into the gate.

`scripts/addr_class_inventory.py` stopped at `perry-runtime/src` and `perry-stdlib/src`, while its sibling `gc_store_site_inventory.py` has globbed `crates/perry-ext-*/src` all along — so the two audits disagreed about what counts as first-party runtime code.

That was not academic. #6826 moved the HTTP server out of `crates/perry-stdlib/src/http.rs` into `crates/perry-ext-http/`, and this gate then reported its 11 handle-floor sites as `baseline says 11, found 0 — lower it to 0`. The sites had not been fixed; they had walked out of the gate's field of view, and the ratchet's own bookkeeping invited someone to ratify that as progress. **A gate whose coverage shrinks silently when code moves** is the failure mode CLAUDE.md's "four ways a gate can be unable to fail" is about, with the twist that here the shrinkage announces itself as a win.

Scanned files go from 826 to **1027**. The 18 newly-visible sites are recorded, not rewritten:

* **10 handle-floor**, baselined per file — `perry-ext-events` (3), `perry-ext-http/agent.rs` (3), `perry-ext-http/lib.rs` (2), `perry-ext-exponential-backoff` (1), `perry-ext-fastify` (1). Five of those are the HTTP server's, i.e. the ones the stale entry was about.
* **8 band-literal**, allowlisted with justifications — each is a wrapper asking "is this f64 payload a small registry handle rather than a heap pointer?" before dereferencing, which is exactly what `addr_class::is_handle_band` answers. They are re-typed rather than called because the ext crates reach perry-runtime through its public surface and `is_handle_band` is not on it. Exporting it is the real fix and belongs with #7448's `ptr_is_tracked_heap_object` export rather than buried in a scan-scope change.

**Verified the widened gate can fail**, rather than assuming it: planting `if (obj as usize) < 0x100000 {` in `perry-ext-exponential-backoff` makes the audit exit 1 and name the file and line; removing it returns exit 0. Worth recording that a first attempt at that sabotage used `p < 0x10000` and did *not* trip the rule — the detector keys on specific shapes, so "I added a magic number and nothing happened" is not evidence the scan is dark.

The sibling `gc_store_site_inventory.py` and `check_file_size.sh` both stay green.
