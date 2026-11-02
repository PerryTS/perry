Group JavaScript exception frames and inline savepoint capture providers to
reduce catch setup overhead. A shared declaration now couples each subsystem's
capture, restore and nested real-throw regression test, while runtime traps
continue to use the C `setjmp` trampoline.
Preserve builtin constructor marker identity across Thin LTO runtime archives.
Keep jump buffers separately aligned and initialize only published snapshots,
avoiding per-frame padding and eager writes to inactive snapshot pages.
