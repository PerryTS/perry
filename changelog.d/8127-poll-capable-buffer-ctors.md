### Fixed

- **`gc-root-dominance` has been red on `main` since 2026-08-14, and the reason was that the gate was silently not checking part of what it claims to.** Its own `--audit-poll-reach` self-audit exits 2 naming five exported symbols that match `ALLOC_RE` (their result is a heap value the checker must track) *and* call something already in `POLL_CAPABLE_RUNTIME`, while not being listed there themselves: `js_buffer_alloc_fill_value`, `js_buffer_from_array`, `js_buffer_from_value`, `js_typed_array_new_from_array`, `js_uint8array_new`.

  The consequence is the #7616 shape: a window whose only collection point is one of those five classified `MOVING: no`, so **every `--moving-only` arm — which is every gated arm, in all four modes — dropped it**. Those windows were never checked, and the gate could not have reported a rooting bug in them.

  All five are real `pub extern "C"` exports in the buffer / typed-array construction paths, and each reaches a JS-reentrant helper (`js_array_get_f64` and `js_typed_array_get` can both run a user getter). Adding them makes the checker **stricter**, not looser: it now classifies those windows as moving and audits them.

  Verified by A/B with real exit codes rather than a pipeline's: `--audit-poll-reach` exits **2** before and **0** after, printing `no ALLOC_RE symbol reaches a poll-capable one unlisted`. `--audit-alloc-re`, `--audit-poll-capable`, `--audit-immovable-sources` and `--self-test` all still exit 0.
