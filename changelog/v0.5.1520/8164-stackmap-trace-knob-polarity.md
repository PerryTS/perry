`PERRY_GC_STACKMAP_TRACE` parses its value instead of its presence, restoring
the `lint` gate. The knob arrived in #8131 reading `var_os(..).is_some()`, which
inverts the spelling a reader is most likely to try: `PERRY_GC_STACKMAP_TRACE=0`
still sets the variable, so it turned the trace ON. `check_gc_env_knobs` — a
required-check audit that exists to keep every GC knob on the shared parser —
failed on it, and a red `lint` on main blocks every open PR.

It now uses `gc::env_flag_enabled`, the default-OFF parser (the same shape as
`policy.rs`'s `PERRY_GC_TRACE`), which fails toward the knob's documented
default so a typo leaves the instrument off rather than silently arming it.
`=1`/`on`/`true` still enable it and unset still leaves it off; `=0`/`off`/
`false`/`no` now disable it.
