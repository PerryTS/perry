# node:fs/promises parity status

The promise suite is tracked separately from `node:fs` because the import surface,
async return values, FileHandle model, and rejection behavior need dedicated
coverage. See `../fs/STATUS.md` for the combined fs/fs-promises coverage count,
reviewed upstream sources, and the follow-up gap list.

## Current coverage

- `node:fs/promises`: 80 fixture files, with 79 parity-pass fixtures and 1 host-Node `node_fail` fixture.
- Full reconciliation run: 79 parity passes, 0 parity failures, 0 compile failures, and 1 host Node `node_fail`.
- Report: `test-parity/reports/parity_report_20260531_231620.json`

The direct submodule manifest rows are present for the runtime-backed promise exports, including `mkdtempDisposable`, `glob`, `watch`, and `constants`. Covered promise fixtures include Node-shaped SystemError metadata (`err.errno`, `err.code`, `err.syscall`, `err.path`, and `err.dest`) for one-path and two-path rejection cases. The remaining unsupported FileHandle tail APIs are `pull`, `pullSync`, and `writer` (#3952).
