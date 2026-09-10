# Next stringify investigation — not implemented

The measured worker calls js_json_stringify_full (not js_json_stringify). On the
R5 linked worker the full entry allocates a 0x110-byte stack frame and saves d9/d8
plus x28..x19 and frame/link registers before checking the inert replacer/spacer
and trying the primitive/string/record paths. See r5-stringify-full-machine.s.
The ordinary small-record fast path is stringify_record_output::try_object,
which can hit emit_repeated_output after exact input/header/epoch verification.
No cause of the 0.67% main regression has been isolated.

Candidate investigation: outline the existing semantic fallback tail after the
four bounded early attempts into a non-inlined internal helper, preserving
exact ordering and all arguments. Inspect final prologue and whole CPU/RSS
matrix; a smaller source function does not itself establish better code.
The current full-entry/worker source is unchanged in R6, which isolates the
index-cache change. Do not bundle this idea into that measurement.
