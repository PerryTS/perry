# node:fs parity status

This split suite replaces the legacy monolithic `test-files/test_parity_fs.ts` and `test-files/test_parity_fs_promises.ts` coverage with granular cases that can be expanded per area.

## Current coverage

- `node:fs`: 108 TypeScript parity cases
- `node:fs/promises`: 54 TypeScript parity cases
- Total: 162 TypeScript parity cases

The suite was built from deterministic behavior in:

- Node's `test/parallel/test-fs*` coverage
- Deno's `tests/unit_node/_fs` compatibility tests
- Bun's Node-compatible `test/js/node/fs` and vendored Node filesystem tests

Covered areas include imports, constants, PathLike Buffer and file URL paths, read/write/readFile/writeFile/appendFile, fd APIs, FileHandle APIs, vector I/O, streams, recursive readdir/opendir, mkdir/rm/rmdir/cp/copyFile, links/symlinks/readlink/realpath, mkdtemp, truncate, chmod/chown/utimes, stats/statfs bigint fields, access modes, glob basics, and watch/watchFile object surface.

## Known follow-up areas

These areas are intentionally left as follow-up work because they require larger runtime behavior or Node-perfect validation semantics:

1. Real `fs.watch`, `fs.watchFile`, and `fs.promises.watch` event delivery, including recursive watching, abort signals, and async iterator behavior.
2. Advanced `glob` semantics: async iterators, arrays of patterns, `exclude`, `withFileTypes`, brace/extglob edge cases, and broader cwd/pathlike validation.
3. Full FileHandle coverage such as `readLines`, readline integration, and more stream lifecycle/error cases.
4. `writeFile` and FileHandle write inputs from streams, async iterables, iterables, and abort signals.
5. Node-perfect `cp` behavior for async filters, exact validation/errors, symlink cycles, subdirectory guards, mode/reflink semantics, and conflict handling.
6. Node-perfect errors across fs APIs: exact error type, `code`, `errno`, `path`, `dest`, and `syscall` fields.
7. Stats `Date` fields (`atime`, `mtime`, `ctime`, `birthtime`) and related timestamp precision. The numeric and bigint timestamp fields are covered; Date object parity needs runtime Date representation work.
8. Stream edge cases: backpressure, `autoClose`, `emitClose`, destroy/error ordering, and fd lifecycle parity.
9. URL/path edge cases, especially full compatibility with `pathToFileURL()`-generated objects.
10. Additional platform- and permission-sensitive behavior once the parity runner can model those deterministically.

## Validation snapshot

Before opening this PR, the split suites passed locally with:

- `./run_parity_tests.sh --suite node-suite --module fs`
- `./run_parity_tests.sh --suite node-suite --module fs-promises`
- `cargo check -q -p perry-runtime -p perry-codegen`
- `git diff --check`
