- fix(codegen): a compact constructor/literal birth stored each argument's
  register from when it was lowered, not the value after later arguments ran.
  `{ _tag: tag(..), annotations: optional(..) }` lowers `tag(..)` first, and
  `optional(..)` can collect. The moving minor rewrote only the argument's
  root, so the new object kept `_tag`'s from-space address. effect's
  `Schema.Struct` then failed with "Cannot read properties of undefined
  (reading 'encoding')" (#12462). The birth now stores the group's re-reads,
  like every other consumer after a collection point. The native-root
  coverage test checks that every dynamic field the fast arm stores is a root
  re-read. Gap test: `test_gap_12462_birth_args_reread`.
- test(runtime): the node_stream rooted-GC witness harness registers the
  native callable-export root scanner. Stream construction reads the minted
  `events.EventEmitter` export, and without that scanner
  `compose_rereads_stages_after_collecting_source_normalization` and
  `readable_map_rereads_chunks_after_collecting_mapper` hit SIGSEGV on
  poisoned from-space. The SIGSEGV killed the rest of the
  `--test-threads=1` runtime suite. The native-root coverage witnesses now
  name the `js_object_alloc_plain` callee that empty literals lower to.
