### Performance

- `JSON.stringify` visits a nested object for about a third of what it did
  (#10696): the per-object cost on nested literals drops from ~2,680 to ~960
  instructions, and `{a:{b:{c:{d:{e:1}}}}}` from 15,748 to 7,338 per call.
  No new side table and no new cache.
  - **One shape probe per object.** The object walk re-derived the receiver's
    keys through the shape table on every key access, about five
    `shape_descriptor_by_id` probes per object. It now resolves keys and the
    live slot bound once (`object_keys_and_live_slot_count`) and re-derives
    them only after a call that can run user code or collect.
  - **Plain object members are walked directly.** A member that is an
    ordinary object with a plain-record class, no meta record, no `toJSON`-ish
    own key and a clean `Object.prototype` verdict skips `member_to_json` and
    `stringify_value_depth`'s dispatch chains, which both ended at the same
    walk. The admission's shape probe and key scan (fused with the
    array-index ordering scan) are handed to the member's walk.
  - **One `Object.prototype` verdict per stretch of callback-free members.**
    The stringify-wide half of the proof is threaded through the walk with
    the shape template's existing `data_record_global_proof` contract and
    cleared before anything that can run user code, instead of revalidating
    the prototype's signature for every object.
  - The same admission serves a compact root object, skipping the root
    `toJSON` lookup, `stringify_value`'s dispatch and `is_object_pointer`. The
    root no longer builds a single-use shape template, and the general root
    path no longer allocates a heap `""` to carry its `toJSON` key.
  - Arrays of records publish an element's index key only when the element's
    own `toJSON` can read it, and format it with `itoa`; `write_number` tests
    integers without a libm `trunc` call.

### Fixed

- A two-or-more-key object literal with a key spelled like a runtime-internal
  field (`__perry_collection_backing__`, …) no longer drops that key from
  `JSON.stringify`: only declared classes can carry those fields, so plain
  records are no longer filtered (a one-key literal already kept it).
