# node:fs/promises parity status

The promise suite is tracked separately from `node:fs` because the import surface,
async return values, FileHandle model, and rejection behavior need dedicated
coverage. See `../fs/STATUS.md` for the combined fs/fs-promises coverage count,
reviewed upstream sources, and the follow-up gap list.

## Current coverage

- `node:fs/promises`: 81 fixture files, with 77 parity-pass fixtures, 4 parity-fail fixtures, and 1 host-Node `node_fail` fixture.
- Full reconciliation run: 77 parity passes, 4 parity failures, 0 compile failures, and 1 host Node `node_fail`.
- Report: `test-parity/reports/parity_report_20260602_140649.json`

The direct submodule manifest rows are present for the runtime-backed promise exports, including `mkdtempDisposable`, `glob`, `watch`, and `constants`. Covered promise fixtures include Node-shaped SystemError metadata (`err.errno`, `err.code`, `err.syscall`, `err.path`, and `err.dest`) for one-path and two-path rejection cases. FileHandle `pull`, `pullSync`, and `writer` are covered for no-transform source/writer paths (#3952); transform pipelines remain outside the current support boundary. Current non-FileHandle parity failures are `arg-validation`, `glob/async-iterator`, `imports/namespace-default-shape`, and `readlink/encoding-pathlike`.
