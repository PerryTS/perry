# Decoder R2 validation on merged main

Base e7223f700c8ce69c388210dab394ad7142550527; candidate source and matched
release binaries are pinned in provenance.json and source.patch. Package 0.5.1529.

- All 283 JSON tests pass, single-threaded, in release mode.
- Matched compiler/runtime-static/stdlib-static build passes (5m19s).
- Raw-handle debt remains 949/949 with 110 module ceilings. Address, runtime-root,
  formatting and file-size checks pass without exceptions.
- All 14 compiled escaped-record scan/sparse comparisons match Node26.5.1.
  Twelve fail against the freshly rebuilt merged-main reference. Before/after
  and oracle output are preserved, along with the validation driver.
- Scheduled scan witnesses 1323 copying minors, 208531 moved objects and 1323
  protected retired sets. Retained results match Node under normal, scheduled,
  protected and full GC; the scheduled case moves 18900 objects.
- 1000 timed changing-input Unicode parses witness 26 ordinary copying minors
  and 12608 moved objects, with retained input/output correctness checked.
- Malloc-sweep trigger counts are 39,40,40,40 for both main and candidate.

These checks establish correctness and live GC witnesses, not timing acceptance.
Performance runs have their own quiet windows and result inventories.
