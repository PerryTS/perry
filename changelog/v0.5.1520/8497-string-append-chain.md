perf(string): fuse proven `s = s + a + b + ...` accumulator chains into one
rooted runtime operation (#8497). The runtime now appends every suffix directly
when a unique accumulator has capacity, or allocates the complete result once,
instead of first materializing the suffix and then entering a separate append
envelope. On `iso_miss`, this reduces instructions retired by 7.91% and cycles
by 9.34% across five shuffled interleaved repeats; no other corpus row moves by
more than 0.41% in instructions. Observed peak RSS changes by +48 KiB (+0.15%).
