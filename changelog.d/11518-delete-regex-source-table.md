perf(runtime): deleted `REGEX_SOURCE_TABLE`; a RegExp is now identified by its
own GC header (#11503). The table was an address-keyed thread-local
`PtrHashMap<usize, RegexMetadata>` whose only payload was
`registered_owner: bool`, yet every RegExp construction inserted into it, every
copying minor rekeyed it, and every collection walked it (once from the
copied-minor from-space pass, once from the sweep-entry
`collect_dead_registered_regexps_post_trace` subphase) just to find dead
RegExps and clear their expandos — work the shared dead-owner fan-out
(`prune_dead_exotic_expando_owners`) already did in the same windows.

- `is_regex_pointer` / `is_valid_regex_ptr` / `is_registered_regex` answer from
  the header alone (`GC_TYPE_REGEXP` + size + `REGEXP_MAGIC`), which they
  already checked first; the table fallback only ever changed the answer for a
  stale entry. Every reader of `registered_owner` was an "is this a regex we
  allocated" membership check — none carried other semantics.
- `GC_TYPE_REGEXP` now uses the shared `GcMoveHookKind::ExoticExpandoOwner`
  move hook and no finalize hook. `GcMoveHookKind::RegExpSideTables`,
  `GcFinalizeHookKind::RegExpSideTables`, the copied-minor regex finalizer, the
  sweep's `dead_regexps` list, the `REGEX_EVER_REGISTERED` latch and the
  now-unused `prefetch_gc_owner_headers` / `exotic_expando_owner_clear_dead`
  helpers are gone. A dead RegExp's expando entry is dropped by the dead-owner
  fan-out; `expando_clear_on_alloc` at construction remains the backstop for an
  owner that died pinned. Block-skip may now reclaim whole dead blocks holding
  RegExps without visiting them.
- Gates: the `REGEX_SOURCE_TABLE` entry is deleted from
  `scripts/gc_runtime_root_holders.json`; `scripts/shape_descriptor_census.py`
  now requires RegExp's type metadata to carry `ExoticExpandoOwner` and
  `GcFinalizeHookKind::None`. (The table was rekeyed by a move hook, not a
  `visit_metadata_*` site, so `gc_rekeyed_key_tables.json` and
  `DEAD_KEY_PRUNES` had no entry for it.)

Tests: `regexp_identity_is_the_header_not_an_address_registry`,
`regexp_gc_type_needs_no_bespoke_side_table_hooks`,
`test_dead_regexp_expando_pruned_on_full_gc`,
`test_live_regexp_expando_survives_full_gc`; the copied-minor
`nursery_regexp_that_dies_young_is_finalized_by_the_copied_minor` and
`test_movable_regexp_evacuation_migrates_all_address_owned_state` now assert
the expando is dropped / migrated (and that a copying minor ran) instead of
reading the deleted table.
