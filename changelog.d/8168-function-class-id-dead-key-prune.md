### Fixed

- **GC: `FUNCTION_CLASS_IDS` kept dead closure keys, so a synthetic class id
  followed whatever object next occupied the address (#8040).**

  `object::class_registry`'s `FUNCTION_CLASS_IDS` maps a synthetic-class
  function value's NaN-boxed **bits** to a class id — i.e. its key is the raw
  heap address of a closure. Both writers (`js_set_function_prototype` for
  `function Base() {}; Base.prototype = {…}`, and
  `synthetic_class_id_for_function` for `Func.prototype.x = fn` / `new Func()`)
  require a `POINTER_TAG`'d value that passes `is_callable_function_value`, so
  a key can only ever have been a live `GC_TYPE_CLOSURE`.

  Nothing told the table when that closure died. `gc::dead_owner` — the
  address-keyed side-table death prune written for exactly this ABA hazard —
  covers around a dozen tables; this one was never wired into its fan-out.
  That is worse here than a leak, because this table is **rekeyed** rather than
  re-derived when its key object moves (`scan_function_class_id_keys_mut`):

  1. the arena recycles the dead closure's address;
  2. the recycled bytes are read as a `GcHeader`, and the byte at the
     `gc_flags` offset happens to carry `GC_FLAG_FORWARDED`;
  3. the rekey walk (`CopyingNurseryCollector::rewrite_raw_addr`) accepts the
     address — `classify_heap_space` correctly reports `NurseryEden`, it really
     is arena memory — and reads the payload word as a forwarding pointer.
     That word is a NaN-boxed value (`0x7FFF…`), so the walk's next hop
     classifies `Unknown` and stops there;
  4. `visit_metadata_nanbox_key` masks the result back to 48 bits, which yields
     a *genuine, live* survivor-space object that is itself being evacuated —
     so the map ends the cycle holding a from-space forwarded address.

  On the production Next.js App Route gate this aborted module init under
  `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1
  PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_SEED=8036` with

      gc evacuation verification failed: stale forwarded pointer in
      runtime mutable root scanner: slot=0x0 old=… forwarded_to=…

  The object behind the offending key was a `GC_TYPE_STRING` carrying
  `GC_FLAG_INTERNED` — a shape a closure-only key can never legitimately name,
  which is what identified the entry as stale rather than mis-rewritten.

  Fix: `prune_dead_function_class_id_keys` joins `gc::dead_owner`'s fan-out on
  both the post-trace (full / fallback-minor) and copied-minor from-space
  paths, using the same `GC_TYPE_CLOSURE`-narrowed deadness predicate the
  closure side tables already use. Live keys are unaffected — they keep being
  rekeyed to their new address by the metadata rewrite pass.

  Regression coverage (`gc::tests::dead_owner_side_tables`, sabotage-verified):
  a dead key is pruned by a full GC; a live key is rekeyed rather than pruned;
  and, under exact Eden reuse, an unrelated closure allocated at the recycled
  address neither inherits the dead function's class id nor has the stale key
  rekeyed onto it across its own move.
