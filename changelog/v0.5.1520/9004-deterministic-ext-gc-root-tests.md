Extension-crate GC root-rewrite tests now select a moving collection through
`perry_runtime::gc::js_gc_force_evacuation_test_override` instead of mutating
`PERRY_GC_FORCE_EVACUATE` process-wide.

These tests assert that a scanner **rewrote** a root, which is only observable
if the collection actually moved the object — and whether a minor evacuates is
a policy decision that legitimately declines under quiet unit-test heaps. Full
CI run 33227701945 failed exactly that way in
`perry-ext-commander::gc_mutable_scanner_rewrites_action_callback_root`: the
scanner was fine, the collection simply stayed non-moving.

The env-var approach it replaces was process-wide, so the forcing window was
visible to every other libtest thread in the binary — the same shape #7946
removed from `perry-runtime`'s own suite, where it silently turned in-place
promotion off underneath `gc::tests::promote_in_place`. The override is
thread-local and tri-state (`1` on, `0` off, negative clears), and each guard
restores the previous state, so a thread that had no override keeps none.

`perry-ext-commander`, `-cron`, `-fastify`, `-streams` and `-ws` were not
forcing at all before this; they now assert against a guaranteed-moving
collection rather than whichever policy the heap happened to pick.
