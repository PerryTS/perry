Add native host-defined resources to typed P2 components. Generated imports
builders receive instance-bound resource factories before initialization, so
imported constructors can create resources backed by Perry values. Ownership
transfers retain payloads across moving GC, invalidate previous owners, preserve
WIT aliases, and reject foreign handles and expired borrows.

Explicit disposal releases payloads immediately, including owned arguments
disposed by an import callback. Destructors run at most once; cleanup continues
after an exception and preserves its original JavaScript value. Failed
initialization and instance disposal also release host payloads held by guests.
GC finalizers still do not invoke application destructors.

The native component-host ABI advances to version 2; rebuild its archive and
the native runtime together. The guest component ABI remains unchanged.

Linux qualification passes 61 targeted tests, 21 native acceptance phases and
34 guest phases, including actual moving GC and native from-space protection.
The execution receipt records the tested source and artifact/log hashes.
