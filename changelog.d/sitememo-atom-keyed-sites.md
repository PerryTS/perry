### Changed

- A computed read site whose entries hold only atom keys (static keys:
  `js_string_pool_atom`, immortal and never moved) or inline short strings
  is no longer listed in `KEYED_SITES`, and the keyed root scan skips such
  keys. Heap string and symbol keys are still rooted.
