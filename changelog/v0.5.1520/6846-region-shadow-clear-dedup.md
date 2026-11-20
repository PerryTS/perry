### Changed

- Region masked-window versioner (`#6794` follow-up (b)): the region fast copies
  no longer re-emit a per-statement shadow-slot clear for a numeric local whose
  slot was already cleared and whose writes are suppressed — those
  `js_shadow_slot_set(slot, 0)` calls are redundant no-ops but each was an
  `_tlv_get_addr` thread-local hit (the dominant runtime symbol in bcryptjs
  `_encipher`). Cuts the ta_i32 fast copy's shadow-slot clears from one-per-write
  to one-per-slot (12→4 on the 16-round `_encipher` shape) with GC roots and
  observable behaviour unchanged (validated under forced evacuation).
