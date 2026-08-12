### Fixed

- **`JSX_NODE_CLASS_ID` and `RAW_JSON_CLASS_ID` were both `0xFFFF_00A0`
  (#7589).** The second class-id collision found in a week, after #7576's
  `ITERATOR_HELPER_CLASS_ID` / `STRING_ITERATOR_CLASS_ID` pair — which had
  silently killed the entire TC39 iterator-helpers surface.

  **Not inert.** `value/to_string.rs` discriminates on `class_id` **alone** — no
  second condition — and returns field 0 as the object's string. A raw-JSON
  wrapper is allocated with one field and keeps its text in field 0, so
  `String(JSON.rawJSON("123"))` matched the **JSX** arm and stringified through
  it. It returned a plausible answer **purely by coincidence of layout**, both
  types happening to store their payload in the same slot. That is the more
  instructive failure mode: a collision can produce right-looking output for
  exactly as long as the two types agree structurally, and break the day either
  layout changes. `RAW_JSON_CLASS_ID` moves to the free `0xFFFF_00A1`; neither
  id is referenced by codegen or persisted, so the move is internal.

  **The gate is the more important half.** #7576 added
  `iterator_class_ids_are_pairwise_distinct`, a Rust test enumerating seven
  iterator ids. It is good and it stays — and it **could not have caught this**,
  because this was a different family and not in the list. That is the
  structural problem with an enumerated list: it covers only the constants
  somebody remembered to add, while the failure being guarded against *is*
  forgetting that a constant exists. **A gate whose coverage depends on the same
  attention the bug depends on is not a gate.**

  `scripts/class_id_collisions.py` scans every crate instead, so a new constant
  is covered the moment it is written, with no list to maintain. It runs in
  `lint` beside the address-classification audit.

  It detects two shapes, and **the second one found itself during development**.
  Different names on one value is the collision. The *same* name on one value is
  a deliberate cross-crate mirror — `perry-ext-events` restates the runtime's
  `ABORT_SIGNAL_CLASS_ID` so it can recognise runtime AbortSignal objects — which
  is correct and must not be reported, or the gate would be permanently red and
  therefore ignored. But one name carrying **different** values is *mirror
  drift*, and that is worse than a collision: each crate stays internally
  consistent, so nothing looks wrong anywhere, while the type silently stops
  being recognised across the boundary.

  Sabotage-verified with exit codes captured directly rather than through a pipe
  (`$?` after `| head` is `head`'s status): reintroducing the collision exits 1,
  drifting the mirror exits 1, raising the stale-scan floor exits 2, clean exits
  0. That floor exists because a scan which silently matches nothing prints "no
  collisions" and means nothing — the same discipline as
  `gc_root_dominance_corpus.sh`'s `MIN_COMPILED`.

  Validation: `JSON.rawJSON` byte-identical to node 26.5.1 across `isRawJSON`,
  `stringify`, the `rawJSON` own property and a nested/mixed array, with JSX
  rendering unchanged as a control; `cargo test -p perry-runtime --lib` 1838
  passed / 0 failed; `addr_class_inventory`, `raw_handle_debt` (998),
  `check_file_size.sh` and `cargo fmt --check` clean.
