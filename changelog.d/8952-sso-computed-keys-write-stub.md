Computed string keys (`o["k" + i]`) stopped defeating every property cache in
the pipeline: **the dynamic-property overwrite loop goes from ~450 ms to
~86 ms — 5.3×**, 8/8 interleaved A/B pairs at stable load, against 28–30 ms
for node on the same host (so ~16× node → ~3.0× node). Binary size +4 KB
(+0.04%); no generated-code growth.

Two changes that only pay off together.

**1. `"str" + n` returns SSO when the result fits.** The fused concat's
NaN-box-returning twin (`js_string_concat_value_box`) assembles results of ≤ 5
ASCII bytes as an inline SSO immediate instead of a fresh `StringHeader`. That
removes one heap allocation per iteration from the hot `"prefix" + i` shape —
but the bigger effect is that the result's BITS become content-stable: `"k" +
42` now yields the identical `f64` every evaluation, so every downstream cache
that compares key *values* can finally hit. ASCII-only, for the same
`utf16_len` soundness reason as `js_string_concat_box`'s existing SSO arm.

**2. A megamorphic stub cache for dynamic string-keyed writes.** A dynamic
write site's inline IC holds `DYN_IC_WAYS = 3`; a loop rotating 500 computed
keys through one site evicts permanently and pays the full miss walk on every
write. Added a thread-local 4096-way direct-mapped cache keyed on
`(shape_token, key_bits)` — V8's megamorphic stub pattern — probed after the
per-site ways miss and fed at every prime site, including for overflow slots
(a wide object keeps *every* data property there, so gating on the inline
region would starve the cache for exactly the receivers it exists for).

Supporting: the SSO→heap materialization every `*const StringHeader` consumer
crosses now interns instead of minting, so a computed key's ADDRESS is stable
too and the address-keyed read plan can hit; short heap keys are folded to
their SSO bits before keying, so a key compares equal to itself across
representations.

**Safety.** The stub stores only content-derived bits, never an address — keys
that don't fit the inline form are rejected rather than cached under their
pointer. That matters: `dyn_ic_try_store` revalidates the receiver's current
shape token, blocking flags and slot bound on every hit, but it confirms the
SHAPE, not that the cached SLOT belongs to this KEY. A pointer-keyed entry
could therefore be primed, evicted from the (direct-mapped, evict-on-collision)
intern table, collected, and its address recycled by an unrelated string whose
write would hit the stale entry and overwrite the wrong slot. Content-only
keying removes that class entirely, and leaves the table holding no GC roots.

**Method note — two wrong verdicts before the right one.** The stub alone
measured as a wash, twice. Counting is what broke it open: 600k inserts, 1.2M
probes, **0 hits**, and splitting the probe-miss counter by cause showed 99% of
misses had the right shape token and the *wrong key* — the keys were fresh heap
pointers each iteration, so a value-keyed cache could never hit. Fixing that
raised hits to 9,584 of 1.19M, still ~1%: the way index XOR'd the low bits, and
an SSO key's low bits are its *first* byte, so `"k0".."k499"` collapsed onto 125
ways with buckets 10 deep and evicted each other continuously. Multiplicative
mixing (480/500 distinct ways, worst bucket 2) is what turned the design into
its measured 5.3×. Neither cause was visible in a profile — only in counters.
