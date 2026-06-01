# node:fs/promises parity status

The promise suite is tracked separately from `node:fs` because the import surface,
async return values, FileHandle model, and rejection behavior need dedicated
coverage. See `../fs/STATUS.md` for the combined fs/fs-promises coverage count,
reviewed upstream sources, and the follow-up gap list.

## Current coverage

- `node:fs/promises`: 79 TypeScript parity cases
- Full reconciliation run: 78 parity passes, 0 parity failures, 0 compile failures, and 1 host Node `node_fail` for `node-suite/fs-promises/rmdir/recursive-and-pathlike`.
- Report: `test-parity/reports/parity_report_20260531_220616.json`

The direct submodule manifest rows are present for the runtime-backed promise exports, including `mkdtempDisposable`, `glob`, `watch`, and `constants`. The remaining unsupported FileHandle tail APIs are `pull`, `pullSync`, and `writer`.
