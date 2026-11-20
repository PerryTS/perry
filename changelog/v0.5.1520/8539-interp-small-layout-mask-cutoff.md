### Performance

- **Cut `interp` retired instructions by 4.61% by keeping tiny mixed GC
  payloads out of the per-object layout tables.** A 30-run sample profile put
  61.1% of self samples in `evalNode`; the result did not support the suspected
  dynamic-numeric-envelope explanation. Instead, its pointer-bearing
  environment arrays were paying to create and later clean up per-object
  layout masks even though those masks could skip only a few exact tag checks.

  Raise the minimum mask-bearing payload from two slots to four, so one-, two-,
  and three-slot payloads use the collector's exact tag scan. A cutoff sweep
  found that four captured the full `interp` instruction win; moving the cutoff
  to eight retired no fewer instructions, while it would make larger objects
  perform extra tag checks and broaden the GC policy change. Across the
  19-benchmark corpus, `interp` moved 9,013,545,066 → 8,598,251,738 retired
  instructions (−4.61%), no other row moved by more than 1%, and peak RSS did
  not regress.
