**Runtime: handle dispatch-extension tables no longer evict a registrant (#11286).**
Native extension crates attach dynamic (untyped-receiver) method, property-get
and property-set dispatch for their handles through
`js_register_handle_{method,property,property_set}_dispatch_extension`
(`crates/perry-runtime/src/object/class_handles.rs`). Each table was a fixed
`[AtomicPtr<()>; 4]` whose overflow path overwrote the last slot. Five crates
register a *method* extension — perry-ext-http (server and client halves),
perry-ext-net, perry-ext-ws and perry-ext-nodemailer — so a program that used
all five silently lost dynamic method dispatch for whichever crate initialized
fourth. Registration is lazy (each crate registers on its first use), so the
victim depended on first-use order. Reproducer: http server, http agent,
`net.createServer`, `new WebSocketServer({ port: 0 })`, then
`nodemailer.createTransport(...)`; an untyped `wss.address()` returned
`undefined` and `wss.close()` was a no-op, leaving the process hung (Node
26.5.1 prints the address object and exits). The property table was exactly
full (4 registrants) and the property-set table held 3.

Each table is now an `ExtensionTable`: the first four registrants still live
in fixed atomic slots, filled strictly in order and scanned exactly as before
(an empty table is one null check on slot 0), and any further registrant is
appended to a growable overflow list consulted only once all four are taken —
nothing is evicted any more. Writers serialize on a mutex; the overflow list is
copy-on-append and published with a `Release` store, and a superseded list is
leaked on purpose (a reader may still be iterating one; the leak is bounded by
the number of distinct registrations). Entries are code pointers, not GC heap
pointers. Registration order is still dispatch priority and re-registration is
still idempotent. A first version used only a growable list; its dispatch loop
cost ~14 more instructions per dispatch than the unrolled four-slot scan, which
is why the inline slots stay.

Tests: `object::class_handles::extension_tests` (unit: nine registrants per
table, each dispatched to) and `crates/perry/tests/issue_11286_dispatch_extension_overflow.rs`
(end-to-end, fails/hangs on the base commit).
