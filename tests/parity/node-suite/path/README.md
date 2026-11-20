# node:path parity suite

Granular Node compatibility cases and retained regression programs for `node:path`.
The broad `test_parity_path.ts` smoke case lives alongside the focused cases.

Run the subject from the repository root:

```bash
python3 scripts/parity.py run node-suite/path --mode auto
```

To regenerate the broad smoke case without recreating its legacy location:

```bash
python3 scripts/gen_parity_tests.py --module path --output-dir tests/parity/node-suite/path
```

Keep migrated `test_gap_` basenames: the universal fast PR lane selects them
across all subject roots. `test_path.ts` retains its basename-based runner skip;
its snapshot entry moves with it rather than turning a skipped case into a pass.
