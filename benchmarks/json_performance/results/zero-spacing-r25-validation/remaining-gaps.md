# Remaining measured gaps after the R25 zero-spacing screen

These are bounded M1 worker comparisons against Node 26.5.1/Bun 1.3.14, not general JSON dominance claims. R25 reference is R24 at the same workspace version; no merged-main timing here.

- Changing record before every plain stringify: Perry 136.790 ns vs Node 107.215 ns and Bun 120.530 ns, including the mutation in all three loops. This is 27.6% more CPU than Node and 13.5% more than Bun. Zero-spacing uses the same canonical path after R25 and now beats both peers; plain construction/emission remains worth profiling independently.
- Small-record pretty stringify: Perry 484.270 ns vs Node 302.888 ns, 59.9% more CPU. Bun is 524.905 ns. R25 does not alter this walk.
- Small-record callback replacer: Perry 947.095 ns vs Node 602.905 ns and Bun 582.635 ns, 57.1%/62.6% more CPU. Callback invocation, holder rooting, side effects and GC make this a separate optimization from a no-callback compact emitter.
- 16 KB pretty record array: Perry 46.292 us vs Node 33.389 us and Bun 45.856 us, 38.6%/1.0% more CPU (Bun difference alone is not statistical evidence).
- R24 read profiles: 1 MB field loop still about 5.8x Node; resolver inclusive samples about 22%, packed index and floating remainder also present. R25 changes only stringify dispatch, so use a matched next experiment before claiming any read gain.
- Existing lazy-array stringify dispatch crashes and noncanonical raw-source output are correctness work. Preserve their failure receipts and isolate fixes; do not enlarge raw-source admission or treat zero spacing as undefined to bypass them.

Next bounded candidate: avoid repeating generic ownership classification for the exact materialized array edge owned and traced by a live lazy header. Preserve real forwarding/length/descriptor/hole semantics, the outlined boundary, and positive copying/protected GC tests. The next-read patch is a proposal only and is excluded from R25 measured evidence.

## Fresh R25 rotating-input parse results

Eight distinct inputs, identical interleaved process protocol; selection overhead measured separately.

- small_record: Perry 0.494836 us vs Node 0.335190 us / Bun 0.255234 us; Perry/Node 1.476, Perry/Bun 1.939.
- records_array_1m: Perry 942.178161 us vs Node 2676.511494 us / Bun 2158.028736 us; Perry/Node 0.352, Perry/Bun 0.437.
- long_string_1m: Perry 99.967589 us vs Node 373.133597 us / Bun 70.525692 us; Perry/Node 0.268, Perry/Bun 1.417.
- escaped_1m: Perry 1011.861635 us vs Node 1725.993711 us / Bun 2078.144654 us; Perry/Node 0.586, Perry/Bun 0.487.
- unicode_1m: Perry 141.954048 us vs Node 439.762217 us / Bun 63.787746 us; Perry/Node 0.323, Perry/Bun 2.225.

Small-record actual parsing remains slower than both peers despite winning the repeated-input microbenchmark. The +0.21% R25-vs-R24 small-record rotating difference rechecked at +0.23%, overlapping ranges and 9/11 pairs slower; roughly 1.1 ns per parse is a disclosed persistent median trend.
