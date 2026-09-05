**The GC census thread-locals no longer pay `_tlv_get_addr`.** `gc::census`
is compiled unconditionally, so its two ungated `thread_local!` blocks were
in shipping builds and cost an out-of-line libdyld call per access on Darwin
— including the `ARMED` flag that gates whether the census does anything at
all. Both now use `crate::perry_thread_local!`, the same treatment the four
other recent declarations received; every call site already went through
`.with()`, so nothing else changed.

This also restores the `self-test-checkers` job, whose thread-local policy
ratchet (#7469) had been failing on main and on every open PR.
