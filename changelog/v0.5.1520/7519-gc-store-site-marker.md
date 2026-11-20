### Gates: restore the GC store-site inventory, red since #7496

`scripts/gc_store_site_inventory.py` went red on `main` when #7496 landed: its
new `element_shape_tests.rs` fills a fresh array's slots with `std::ptr::write`
to imitate the way an inline array literal's codegen fills an allocation, and
that raw store carried no `GC_STORE_AUDIT` marker.

The store is deliberate and the test depends on it — routing it through a
barriered helper would run the very funnel the test exists to prove is absent,
so the fix is the marker, not the helper. Annotated `GC_STORE_AUDIT(INIT)`
with the reasoning: the array is nursery-fresh, never escapes the test, and
`ensure_element_shape` is what must then self-heal the invariant.

Found by running the static gate battery over `main` directly rather than
waiting for CI.
