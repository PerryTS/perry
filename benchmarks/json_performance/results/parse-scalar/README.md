Initial scalar-parse candidate, retained for reproducibility.

The full matrix passes the quiet gate, with 152 output verifications, 456 timing
trials and 432 memory trials. The paired check contains 154 trials and passes
its separate quiet gate. Source hashes and worker hashes are in provenance.json.
Decompress and apply initial-source.patch.gz to efc789c72467e0304a19043af249a16318756d95 to reproduce
the measured source.

Review subsequently found that successful scalar parses skip the existing
oversized parse-key cache cleanup after lazy materialization or typed-key setup.
This dataset describes that initial candidate, not the corrected implementation.
The paired check also confirms small-object parse regressions (0.7–2.4%) against
the tape-scanning checkpoint. This is not an accepted no-regression result.
