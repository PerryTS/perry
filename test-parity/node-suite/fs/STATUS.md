# node:fs parity status

This split suite replaces the legacy monolithic `test-files/test_parity_fs.ts` and `test-files/test_parity_fs_promises.ts` coverage with granular cases that can be expanded per area.

## Current coverage

- `node:fs`: 169 TypeScript parity cases
- `node:fs/promises`: 79 TypeScript parity cases
- Total: 248 TypeScript parity cases

The suite was built from deterministic behavior in:

- Node's `test/parallel/test-fs*` coverage
- Deno's `tests/unit_node/_fs` compatibility tests
- Bun's Node-compatible `test/js/node/fs` and vendored Node filesystem tests

Covered areas include imports, export-tail namespace coverage (`Dir`, `Dirent`, `Stats`, `ReadStream`, `WriteStream`, `FileReadStream`, `FileWriteStream`, `Utf8Stream`, `_toUnixTimestamp`, `openAsBlob`, `mkdtempDisposableSync`, `constants`, `promises`), constants, PathLike Buffer and file URL paths, read/write/readFile/writeFile/appendFile, fd APIs, FileHandle APIs, vector I/O, streams, recursive readdir/opendir, mkdir/rm/rmdir/cp/copyFile, links/symlinks/readlink/realpath, mkdtemp and disposable temp dirs, truncate, chmod/chown/utimes, stats/statfs bigint fields, access modes, advanced glob options and async iteration, deterministic watch/watchFile event delivery, and Node-shaped argument validation for the covered fs/fs-promises cases.

## Known follow-up areas

These areas are intentionally left as follow-up work because they are outside the deterministic fs parity slice or remain unsupported API tail:

1. Remaining `fs.promises.FileHandle` export-tail APIs: `pull`, `pullSync`, and `writer`.
2. `fs.Dir` concurrent-operation overlap semantics: Node throws `ERR_DIR_CONCURRENT_OPERATION` when `readSync()` or `closeSync()` is attempted while an async `read()` or `close()` is already in progress. Tracked by #3964 with DeepWiki evidence in `reference/deepwiki/nodejs-node-fs-dir-prototype-close-parity.md`.
3. Platform-specific permissions and ownership behavior: Windows `chmod`/`chown` limitations, POSIX-only permission-denied branches, symlink behavior, reserved Windows path characters, and host filesystem differences remain documented rather than forced into the default deterministic run.
4. `fs.watch`, `fs.watchFile`, and `fs.promises.watch` now have deterministic event-delivery, recursive, abort, and async-iterator coverage. Node's documented platform quirks remain out of scope for default parity: inode replacement on Linux/macOS, Windows rename/delete behavior, missing `filename`, network filesystem unreliability, and unsupported platforms.
5. `copyFile` and `cp` now cover async filters, option validation, conflict handling, symlink/subdirectory guards, and reflink/mode acceptance in curated fixtures. Node still documents copy operations as non-atomic, and failed-copy destination cleanup cannot be made deterministic across all host filesystems.
6. Callback-style fs APIs invoke `cb(err, …)` with a real `Error` carrying `err.code` (`"ENOENT"`, `"EACCES"`, `"EEXIST"`, …), `err.syscall`, and `err.path`; promise APIs reject with Node-shaped validation errors for the covered cases. Errors raised inside lower-level syscall paths that bypass the typed wrapper may still need broader typed-error propagation through LLVM.
7. On POSIX, `ctime` is read from `MetadataExt::ctime` (plus `ctime_nsec`) and the bigint `atimeNs`/`mtimeNs`/`ctimeNs` fields use real `*time_nsec` counters, so sub-millisecond precision is preserved. Windows still falls back to the millisecond x 1e6 approximation.
8. `mkdtemp` returns an empty path on exhaustion after 64 collision retries instead of throwing. Once typed error propagation lands, promote this to a real ENOSPC/EACCES rejection.

## Validation snapshot

Final reconciliation evidence:

- `./run_parity_tests.sh --suite node-suite --module fs` -> 169 parity passes, 0 parity failures, 0 compile failures, 1 host Node `node_fail`, report `test-parity/reports/parity_report_20260531_231300.json`.
- `./run_parity_tests.sh --suite node-suite --module fs-promises` -> 79 parity passes, 0 parity failures, 0 compile failures, 1 host Node `node_fail`, report `test-parity/reports/parity_report_20260531_231620.json`.
- `target/release/perry --print-api-manifest=json` includes current `fs` and `fs/promises` rows and does not include unsupported `pull`, `pullSync`, or `writer` rows.
- `test-parity/known_failures.json` has no fs or fs-promises entries.
