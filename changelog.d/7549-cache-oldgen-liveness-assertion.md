### Tests: assert the sparse cache is old-gen before testing the containment branch

#7546's external-edge test asserted that the lazy array's sparse cache and its
header land on different pages, but never that the cache block is **old-gen**.

`slot_is_external_to` short-circuits to `true` on generation *before* it
reaches the containment test. So if an allocator change ever moved a 32 KiB
sparse cache into the nursery, the containment branch this test exists to cover
would quietly stop running — and the test would still pass. That is CLAUDE.md's
"a gate must assert its subject was live" hazard, applied to a test that was
itself written to close a coverage hole of exactly that kind (#7500's
`test_dirty_lazy_array_external_cache_scan_marks_bitmap_selected_child` was
green for its whole life because no producer ever wrote a real entry).

Adds the missing precondition. Raised by CodeRabbit on #7546; applied here
because the reviewing agent's worktree was removed before it could.
