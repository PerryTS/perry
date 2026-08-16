### Performance

**Property-get IC misses no longer ask the Set and Symbol registries about an
ordinary object (#7867).** `get_field_by_name_object_tail` already classified
Buffer and TypedArray before reading the receiver's `GcHeader`, then switched
on that header for the rest of its exotic-receiver cases. Set and Symbol were
the exceptions: every miss consulted their address-keyed registries first,
including the process-global Symbol mutex and hash table.

Those two exceptions need different gates. Sets are allocated with
`GC_TYPE_SET`, so the header can rule them out before `SET_REGISTRY` is touched.
Symbols span two storage classes: fresh `Symbol()` values are GC-backed strings,
while `Symbol.for`, well-known, and Intl symbols are Box-leaked and have no
`GcHeader`. Every Symbol does carry `SYMBOL_MAGIC` in its first word, so the
existing exact-false `may_be_symbol_header` screen rules ordinary receivers out
without excluding either storage class. In both positive cases the registry
remains authoritative, including for stale address reuse.

The ordering remains deliberate. Buffer and TypedArray are headerless and stay
before the first `GcHeader` read. A possible headerless Symbol returns from the
magic-gated dispatch before that read. Only then does the tail validate the
pointer, read the header once, and use `GC_TYPE_SET` to decide whether Set
dispatch is possible.

The registered-receiver bodies are `#[cold]` and `#[inline(never)]`. This is not
cosmetic: an initial version left them inline, grew
`get_field_by_name_object_tail` by 40 bytes, shifted every following runtime
function, and regressed `pipeline_big` by **+4.636%** paired geomean (30
alternating quiet-M1-mini pairs, 95% CI +4.589% to +4.681%). Sampling showed no
new hot leaf inside the changed tail; the regression tracked the text-layout
shift. That version was rejected rather than hidden behind the intended probe
saving.

With the cold dispatch outlined, the same locked-host A/B measured 30
alternating pairs at 1.693549 s base median and 1.688705 s fixed median:
**-0.315% paired geomean**, bootstrap 95% CI -0.355% to -0.273%. Both arms
exited zero and produced the exact `pipeline_big` oracle output
`556260000 3 3`.

The issue's earlier 1.8% `is_registered_symbol_slow` attribution is no longer
present on current `main`: fresh debug-symbol samples contain zero such samples
in either arm. The structural cost is still directly reproducible. The new
test arms both registries, performs plain-object misses, and asserts the
test-only entry counters do not move; before this change the Set counter moves
from 801 to 803. It then sabotages the Symbol magic screen after a successful
lookup to prove the Symbol dispatch actually ran and the Set registry remained
untouched. A companion test exercises Set `.size`, a Box-leaked
`Symbol.for(...).description`, and a GC-backed `Symbol(...).description`, so
deleting a positive path or accidentally requiring every receiver to have a
header turns the suite red.

Validation includes the full serialized runtime suite (2,154 passed, 4
ignored), its doc tests, test-registration and thread-local policy checks, the
address-class audit, formatting, file-size, and whitespace gates. The
address-class audit also reports the pre-existing stale `dyn_index.rs` ratchet
entry (`baseline 2, found 1`); that adjacent cleanup is intentionally not folded
into this PR.
