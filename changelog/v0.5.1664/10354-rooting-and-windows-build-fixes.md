**A moving-GC use-after-move in nodemailer's send path, a broken Windows
child-process build, and three ratchets that were ours.**

`nodemailer`'s `info_object` — the `{ messageId, response }` object a
successful `sendMail` resolves with — held the freshly allocated object as a
bare `*mut ObjectHeader` across two `js_string_from_bytes` calls. Either can
collect, and a collection can MOVE the object, so each following
`js_object_set_field` could write through a pointer that had already been
relocated. This is the #7210 shape: nothing fails at the site, and the damage
surfaces cycles later, in unrelated code, as
`TypeError: value is not a function`. All three values are rooted in a
`RuntimeHandleScope` now and re-read through their handles.

Found by `scripts/unrooted_local_shape.py`, not by a test — which is the point
of that ratchet. A second finding in `perry-ext-http`'s
`build_listen_error_value` was safe in fact but not by construction: its
safety rested on a helper in another function only ever building a Rust
`String`. `obj` is rooted before anything in the function can allocate now,
and the alloc-failed path moved into its own function, because that checker is
flow-insensitive BY DESIGN — a window it cannot rule out is one a reader
cannot either.

**Windows:** `cp_register_windows_live_child` handed
`cp_register_live_child_parts` a boxed reader for stdout/stderr, but that
function takes `Option<CpPipe>` — the pipe is stored as an owned handle and
only becomes a reader at read time. macOS and Linux never compile this
`cfg(windows)` arm, so every local build and `cargo check --all-targets` was
green while the Windows job died in "Build compiler and static runtime",
before the probe matrix it exists to run. The neighbouring `CpStdin::new` call
is NOT the same defect and is unchanged: that function has two cfg'd forms,
and the one-argument version is correct for Windows.

Two ratchets besides the rooting one, both green on `main` and therefore ours:
`turnloop_pool/tests.rs` open-coded `ptr + size_of::<StringHeader>()` instead
of `OwnedStringBytes::copy_from_header` (the payload offset is the runtime's
to know), and eight `ptr::write` sites in `turnloop_net/abi.rs` were unmarked
— all of them writing scalars or `&'static str` pointers into a CALLER-supplied
C out-parameter rather than a GC slot, now marked `GC_STORE_AUDIT(POINTER_FREE)`
with a per-site reason. `turnloop_post`'s `DISPATCHED` counter became a
`perry_thread_local!` rather than being recorded as cold debt: it ticks on
every posted-job dispatch and is the liveness counter posted-job tests read,
so an allowlist entry would have recorded the opposite of the truth.

Unrooted-local recorded debt falls 428 → 390.

Also repairs the P3 deadline-agreement test, which was the ROOT of seven
reported `event_pump` failures — the other six were `PoisonError` cascades
from this one panicking on a spawned thread and poisoning a shared mutex.
`js_set_timeout_callback` returns the handle OBJECT's pointer since #340/#341,
not the registry id, so the test's `clearTimeout(id)` cleared nothing. It
boxes the pointer and goes through `js_clear_timeout_value`, which is also the
entry point codegen emits for JS `clearTimeout`, so the shipped path was never
affected.
