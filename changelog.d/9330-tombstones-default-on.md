### Performance

- **Tombstone deletes are default-on again.** #9038's O(1) delete (6.5× on
  populated delete, ~15× vs node at the time) was rolled back to opt-in by
  #9212 because of #9200 — an evacuating minor could sweep a deleted
  receiver's live keys array, leaving it shapeless. #9317 fixed that
  structurally (every post-birth ShapeId publish arms `old_carrier` through a
  single funnel), so the win returns to the default configuration.
  `PERRY_OBJECT_TOMBSTONES=0` remains the kill switch.
