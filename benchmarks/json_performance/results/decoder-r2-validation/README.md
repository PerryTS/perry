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

The original full validation driver is preserved byte-for-byte in
`validate-recorded.txt`; it records the local build procedure and is not a
portable entry point. `validate.py` now replays the compiled escaped-record
checks from any working directory, resolves Node from PATH / `NODE` / `--node`,
and enforces the repository's exact `.node-version` pin. Stage the immutable
workers first, then run:

```sh
python3 benchmarks/json_performance/results/decoder-r2-validation/validate.py \
  --results-dir benchmarks/json_performance/.work/decoder-finite-replay
```

The original last fixture lacks `id`; its serialized-output comparison is valid,
but its scan/sparse checksum is NaN and was not checked. The
[finite-checksum replay](../decoder-r2-finite-validation/README.md) adds that field
and verifies every numeric result without rewriting the historical measurements.
