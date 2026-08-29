The write barrier's dirty-page cache is now sixteen direct-mapped ways instead
of one entry.

The single entry was justified by a `batch.ts` simulation whose stores arrive in
long same-page runs. The ECS component-update rows falsify that shape: each
entity's sweep stores into every component column in turn, so the store pages
alternate and one entry can never hold them. Every store then took the uncached
path — `mark_dirty_old_page_uncached` plus the `DIRTY_OLD_PAGES` thread-local
resolution measured 35–40% of both update rows' frames.

Ways are indexed by the page number's low bits. That is safe here precisely
because page numbers are `addr >> 12` and therefore sequential, so neighbouring
columns land in distinct ways — unlike the dynprop case where low-bit folding
collapsed keys. A hit bypasses the uncached path entirely, so the thread-local
resolution and the hash insert stop executing rather than getting cheaper.

The safety invariant is unchanged and still one-directional: a way answers
"already marked" only on an exact page match, so a stale way answers "not
marked" and the store takes the recording path. `invalidate()` clears every way
on the same removal paths as before, so the cache can still only suppress a
REPEAT recording, never a first one.

The #8949 process-global mirror is retired rather than widened: it existed to
shave the single cell's dependent-load chain for +0.17%, and a multi-way hot-TLS
map supersedes both its mechanism and its rationale.

`HotTls` grows by 120 bytes at `dirty_old_pages`, which sits after both offsets
codegen hardcodes (`inline_state`, `implicit_this`), so the emitted offsets are
unaffected — pinned by `hot_tls_layout_is_what_codegen_assumes`.
