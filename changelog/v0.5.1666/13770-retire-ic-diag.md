`PERRY_IC_DIAG` is retired. Its counters (`ic_note`, `ic_note_prime`,
`ic_note_hit`, `js_ic_diag_note_hit`) had no caller left in the runtime or in
emitted code, so an armed run wrote an empty report. The instrument, its knob
in the hot-diag lists (runtime, compiler, build-cache test), its wasm ABI row,
its GC root-holder entry and its uses in the miss-read lane scripts are
removed. `PERRY_REGEX_DIAG`, `PERRY_ENUM_DIAG` and `PERRY_RECEIVER_REPR_DIAG`
are unchanged.
