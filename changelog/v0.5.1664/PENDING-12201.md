Remove the implicit descriptor route from unclassified GC cells into the
native handle expando registry. Property holders route explicitly to their
existing owned bags; forwarded ordinary objects resolve to the live holder.
Internal capture boxes, weak storage and opaque native payload cells refuse
descriptor routing and diagnose invalid callers in debug builds. No new cache,
latch or side table is introduced.

Exercise every JS holder representation with defineProperty and reflection,
including native fetch-band handles. Fix their Linux admission into the native
handle route instead of the broad object-pointer path. Native registry ids and
native Box backings retain explicit stable-owner storage and release cleanup;
GC payload cells never enter that address-keyed registry. Add refusal tests
for both installs and queries, an obsolete-registry collision, forwarded-object
aliases and lazy-array normalization.

Add sloppy/strict writes to non-writable array indices and named properties,
and reflection of frozen array named properties. Remove the unused Error
property-removal helper; keep the getters, setter and enumeration helper used
by filesystem errors, REPL and formatting. Correct the array accessor comment
to describe facts in holder shapes and pairs in holder slots.

Allow holey arrays to grow densely when their index descriptor keys are already
inside capacity; genuine sparse indices beyond capacity still block growth.
Defer the reserve-word discriminator optimization: a low pointer bit needs a
collector slot representation that masks it for tracing/barriers and preserves
it on evacuation and rewrite. Decoder-only tagging would corrupt the existing
ordinary NaN-box pointer edge. A future change should introduce that slot
contract and verify bag/pairs/inline transitions, growth and moving GC together.
