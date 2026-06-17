# npm compilePackages sweep

This directory owns the advisory sweep for #805. It is intentionally separate
from `tests/release/packages/`: release fixtures are pinned, deterministic
gates, while this sweep follows current npm package drift and records a trend
signal.

The runner creates a temporary fixture per package, installs the package,
compiles a tiny namespace import with `perry.compilePackages`, optionally runs
the produced binary, and writes:

- `results.json` with one structured row per package
- `results.csv` with the same rows in trend-friendly form
- `summary.md` for GitHub Actions step summaries
- per-package logs under `logs/<package>/`

The scheduled workflow uploads those files as artifacts. Package failures are
expected while compatibility is incomplete, so the workflow is advisory by
default and does not run on pull requests.

## Local usage

Build Perry first:

```sh
cargo build --release -p perry-runtime -p perry-stdlib -p perry
```

Run the default tier:

```sh
python3 test-compat/npm-sweep/run.py \
  --perry-bin target/release/perry \
  --out-dir .npm-sweep-results \
  --history test-compat/npm-sweep-history.csv
```

Run a dry plan without npm or Perry:

```sh
python3 test-compat/npm-sweep/run.py --dry-run --packages nanoid,ms,zod
```

Manual package selection accepts comma-separated specs:

```sh
python3 test-compat/npm-sweep/run.py --packages express@latest,@types/node@latest
```

Use `--strict` only when intentionally turning the sweep into a gate; without
it, package failures are recorded and the runner exits zero.
