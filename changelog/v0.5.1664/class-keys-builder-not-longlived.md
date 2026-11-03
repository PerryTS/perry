Fixed stale young-key pointers detected during OpenCode TUI initialization.
Building a class keys list for a class with a dynamic parent copied the parent's
young strings into a Longlived source array. Canonicalization kept a separate
array, but the temporary source remained immortal. Longlived objects are neither
barriered nor swept, and a minor only rewrites them when traced, so their young
words could remain stale after evacuation. The builder now allocates its source
array and new key strings in the nursery. A deterministic arena-walk witness
fails when the source array is moved back to Longlived. This fixes a verified
heap invariant violation; its causal connection to the intermittent
`[gc-pin-latch] FATAL ... header coherence INCONSISTENT` abort has not been
established.
