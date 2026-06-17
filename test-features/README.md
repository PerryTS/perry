# TypeScript Feature Matrix

This directory contains a small compatibility radar for TypeScript and modern
JavaScript language features. Each probe is a standalone `.ts` file that Node
runs with `--experimental-strip-types`; Perry compiles the same file to a
native binary and the generator records whether stdout matches.

Regenerate the committed matrix after adding probes or intentionally changing
language behavior:

```bash
python3 scripts/gen_feature_matrix.py --perry-bin target/release/perry
```

Check that the committed matrix is current:

```bash
python3 scripts/gen_feature_matrix.py --check --perry-bin target/release/perry
```

Current failures are allowed in `feature_matrix.md`; the CI check only detects
drift between the probes and the committed baseline.
