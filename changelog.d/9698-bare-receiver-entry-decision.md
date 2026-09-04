**A dynamic method call on a *bare* managed receiver — a real GC pointer whose
value was never NaN-boxed — was rooted under a tag the collector ignores, and
reboxed as an object even when it was a string.** `js_native_call_method` did
recover the shape, but only in the last few lines of the ~1200-line dispatch
tower and only ever as `POINTER_TAG`. Three defects followed, and a `.slice`
on a value that must be a string is what they surface as (#9675).

**1. The receiver was not a root.** The tower parks its receiver in a
`RuntimeHandleSlot::Nanbox` via `RuntimeHandleScope::root_nanbox_f64`. That slot
kind is marked by `gc::try_mark_value` and rewritten by
`gc::try_rewrite_nanboxed_value`, and *both* begin by rejecting any word whose
tag is not `POINTER_TAG`/`STRING_TAG`/`BIGINT_TAG`. A bare pointer's tag is
zero, so rooting one there neither **marks** the receiver — a full mark-sweep
inside dispatch can reap it — nor **rewrites** the slot — an evacuating minor
leaves it holding a from-space address. This is #6910's hole re-opened in a
different registry: that issue fixed exactly this mismatch for shadow-stack and
global-root slots (`gc/tests/root_words.rs` pins it, "bare address" included),
and the transient-handle registry's nanbox slot kind was never converted.
#7528's fix — re-read the root slot at every use instead of caching a local —
is necessary but not sufficient here, because re-reading a slot the collector
never rewrote returns the same stale address.

**2. Strings and BigInts came back as objects.** `JSValue::pointer` stamps
`POINTER_TAG` unconditionally. `string_methods::dispatch_string` gates on
`is_any_string()`, which accepts only `STRING_TAG`/`SHORT_STRING_TAG` — so a
bare `GC_TYPE_STRING` receiver was reboxed into something that is not a string
and `.slice` never reached the string arm.

**3. No forwarding walk.** A bare receiver a collection had already moved was
reboxed at its from-space address.

**4. And the mirror image: an unvouched bare word was still read as a pointer,
and faulted.** A genuine positive subnormal double has bits that look like an
address — `1e-310` is `0x1268_8b70_e62b`, above the handle band and inside
`is_valid_obj_ptr`'s platform window. The tower's probes are magnitude-gated:
`try_read_gc_header` classifies by address range and then dereferences
`addr - GC_HEADER_SIZE`, a contract written for a *stale* heap address, where
the page is still mapped. An arbitrary number is not a stale address; nothing
was ever mapped there. So `(1e-310 as any).toString()` **SIGSEGV'd** inside
`url::search_params::shape_is_url_search_params` — a probe that is itself
careful (it gates on `try_read_gc_header` precisely so a `Date` cell cannot
fault it) and simply cannot tell a number from an address. Node prints
`1e-310`. A smaller subnormal aliases the *handle* band instead: `5e-324` is
`0x1`, so the handle dispatcher answered it and `(5e-324 as any).toString()`
returned `undefined` where node returns `"5e-324"`. Both were found while
validating this change, both reproduce on unfixed `main`, and both are fixed
here — the backtrace was taken rather than assumed, and it named a probe
nobody would have guessed.

`canonicalize_bare_gc_receiver` now runs as the first statement of
`js_native_call_method`, before the root and before the first probe. The gate is
**allocator ownership, not address magnitude**:
`value::addr_class::try_read_tracked_gc_header` requires arena page membership
or an exact malloc-registry hit, plus a valid `obj_type`/`size`/arena-flag
triple, before the first header byte is touched. Forwarding is then followed
through `value::resolve_forwarding`, and the tag is chosen from the resolved
header: `STRING_TAG` for `GC_TYPE_STRING`, `BIGINT_TAG` for `GC_TYPE_BIGINT`,
`POINTER_TAG` otherwise. `GC_TYPE_STRING` is also what `alloc_symbol`
gc_mallocs, so the string arm carries the same `SYMBOL_MAGIC` content screen
`gc_pointer_and_type_from_value` uses — a fresh `Symbol()` keeps `POINTER_TAG`,
which is how symbols are boxed.

The allocator is not the only owner that can answer without touching memory, so
the gate also asks the address-keyed registries that own **headerless**
allocations — a `Symbol.for` symbol, an `ArrayBuffer`/`Uint8Array` backing
store, a typed array. Those have no `GcHeader` for the allocator gate to find,
are boxed as POINTERs, and are exactly the case the old tail recovery
legitimately served. Each is a table lookup behind an idle latch, and each is
already consulted by `gc_pointer_and_type_from_value` for the same reason.

Once every owner has been asked and declined, the word is *definitively not a
managed pointer*, so it is the number its bits spell and
`dispatch_unvouched_bare_as_number` answers it there and then — it never reaches
a pointer-shaped probe. Fixing only the probe that happened to fault would have
been whack-a-mole; every magnitude-gated probe in the tower has the same
exposure, and CLAUDE.md's "Known-weak areas" records this codebase paying for
that pattern three times over. Deciding once, at the chokepoint, makes the class
unreachable instead of fixing its current instance.

That also makes the tower's magnitude-only **tail recovery unreachable**, and it
is deleted here: it was the last place that treated address magnitude as proof,
it did neither of the two things the entry now does (correct tag, and a root the
collector traces), and its one legitimate constituency — headerless registry
allocations — is served by the vouch set above.

Every NaN-boxed receiver returns from one `bits >> 48` compare, so the ordinary
dispatch path is unchanged. `+0.0` is deliberately left on the ordinary path: it
is not address-shaped, so no probe can mistake it for a pointer.

**What the pre-fix path actually did**, measured by flipping the new guard off
and re-running the tests: `.slice(1)` on a bare managed string receiver did not
throw — it returned a `POINTER_TAG` value whose payload
`try_read_tracked_gc_header` does not recognise at all, at a *different address
on every run*. A wild pointer handed back to codegen, which NaN-unboxes and
dereferences it. Seven of the eleven new tests fail on that arm; all eleven pass
with the fix.

`native_call_method/bare_receiver/tests.rs` covers both directions, because the
second is what keeps a gate like this honest. Reclassification: string →
`STRING_TAG` (and satisfying the exact `is_any_string()` predicate
`dispatch_string` gates on), BigInt → `BIGINT_TAG`, plain object/array →
`POINTER_TAG`, a hand-forwarded receiver → its *current* address, and an
end-to-end `js_native_call_method(bare, "slice", [1])` returning `"bcdef"`.
Non-reclassification: genuine positive subnormal doubles whose bits sit squarely
inside the platform heap window the old tail recovery accepted, a `Box`
allocation carrying a hand-built `GcHeader`, headerless registry handle ids
across every band, a fresh `Symbol()`, and twelve NaN-boxed forms returned
bit-for-bit. Routing: the two subnormals that actually bit (`1e-310`, `5e-324`)
must reach number dispatch; a `Symbol.for` symbol — asserted in the test to be
registered *and* not allocator-tracked, so the registry arm is load-bearing
rather than incidental — must not; neither must any allocator-vouched receiver;
and `+0.0` must stay on the ordinary path.

Rejected alternative: root the receiver with `root_heap_word_u64` instead, whose
`HeapWord` slot kind *does* accept bare addresses (that is #6910's fix). One
line, but it closes only defect 1 — strings would still dispatch as objects and
a forwarded receiver would still be used at its old address — and it would move
every ordinary receiver onto the more conservative `try_mark_value_or_raw`
interior-pointer path on the hot dispatch route.
