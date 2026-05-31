Work through this checklist by dependency readiness. Fully implement one ready task before moving to the next. Respect After: dependencies. Preserve existing SQLite compatibility unless Node builtin parity requires a scoped wrapper change. Validate each task against the cited issue before replying. Keep updates concise and implementation-focused.
-----
- [x] node-sqlite-custom-functions-authorizers - Add custom functions, aggregates, defensive mode, and authorizers
  Problem: `DatabaseSync` lacks Node-compatible SQL callback/control hooks for `function`, `aggregate`, `enableDefensive`, and `setAuthorizer`.
  Context: Implement callback argument/value conversion, authorizer constants, allow/deny/ignore handling, and validation without changing unrelated database APIs.
  Reference: https://github.com/PerryTS/perry/issues/3185; DeepWiki: repo=`nodejs/node`, question=`How does node:sqlite implement DatabaseSync function, aggregate, enableDefensive, setAuthorizer, callback argument conversion, authorizer constants, and validation errors?`
  Acceptance: Tests register scalar and aggregate SQL callbacks, verify authorizer allow/deny/ignore behavior, defensive mode, and invalid argument errors.

- [x] node-sqlite-sessions-constants - Implement sessions, changesets, applyChangeset, and constants
  Problem: Perry has no Node builtin `Session` object, changeset/patchset behavior, `applyChangeset`, or `sqlite.constants` namespace.
  Context: `db.createSession()`, `Session#changeset`, `Session#patchset`, `Session#close`, disposal, `db.applyChangeset`, and changeset/action/authorizer constants.
  Reference: https://github.com/PerryTS/perry/issues/3186; DeepWiki: repo=`nodejs/node`, question=`How does node:sqlite implement createSession, Session changeset/patchset/close/disposal, applyChangeset, and sqlite.constants for changeset and authorizer behavior?`
  Acceptance: Parity tests create sessions, mutate tracked tables, return `Uint8Array` changesets/patchsets, apply changesets to a matching database, and verify close/disposal errors and constants.

- [x] node-sqlite-finalize-dirty-scope - Finalize the dirty custom-function and Session SQLite slice
  Problem: The remaining SQLite feature work is implemented but unstaged, with two untracked parity tests and unchecked task state.
  Context: `crates/perry-stdlib/src/sqlite.rs`, runtime/native-module dispatch, HIR/codegen routing, `Cargo.lock`, and `test-files/test_parity_sqlite_custom_functions_authorizers.ts` / `test_parity_sqlite_sessions_constants.ts`.
  Reference: Existing tasks `node-sqlite-custom-functions-authorizers` and `node-sqlite-sessions-constants`.
  Acceptance: The two existing tasks are verified, checked off, and committed with no untracked SQLite feature files left.

- [ ] node-sqlite-regen-api-docs - Regenerate API docs for node:sqlite manifest changes
  After: node-sqlite-finalize-dirty-scope
  Problem: `crates/perry-api-manifest/src/entries.rs` changed, but generated API docs are not updated.
  Context: `scripts/regen_api_docs.sh`, `docs/api/perry.d.ts`, and `docs/src/api/reference.md`; CI has an API docs drift guard.
  Reference: `.github/workflows/test.yml` API docs drift job.
  Acceptance: Regenerated docs are committed and `git diff` is clean after rerunning the docs generator.

- [ ] node-sqlite-clear-stale-parity-gaps - Remove stale SQLite known-failure and gap inventory
  After: node-sqlite-finalize-dirty-scope
  Problem: `test-parity/known_failures.json` and `docs/runtime-parity-gaps.md` still describe implemented `node:sqlite` surface as missing.
  Context: `test_parity_sqlite` output currently matches Node locally, and the new focused SQLite outputs should match Node on the current fixtures.
  Reference: Existing SQLite parity reports and output files under `test-parity/output/{node,perry}/`.
  Acceptance: Stale SQLite known-failure/gap entries are removed or narrowed, and the full SQLite parity filter passes without hiding implemented APIs.

- [ ] node-sqlite-session-build-portability - Prove the SQLite session build is portable
  After: node-sqlite-finalize-dirty-scope
  Problem: Enabling `rusqlite`'s `session` feature pulls in `libsqlite3-sys` bindgen/libclang and new SQLite session FFI symbols.
  Context: `crates/perry-stdlib/Cargo.toml`, `Cargo.lock`, `ffi::sqlite3session_*`, `ffi::sqlite3changeset_apply`, and clean Linux/macOS CI images.
  Reference: Session feature dependency changes in the current dirty worktree.
  Acceptance: Either CI/base images are proven to build the session feature, or the implementation removes the extra bindgen/libclang requirement while `cargo check -p perry-stdlib --features database-sqlite` passes.

- [ ] node-sqlite-main-rebase-proof - Rebase and verify SQLite against current origin/main
  After: node-sqlite-regen-api-docs, node-sqlite-clear-stale-parity-gaps, node-sqlite-session-build-portability
  Problem: The branch is dirty and far behind `origin/main`; fresh build and parity evidence must be captured after conflict resolution.
  Context: Rebase the five committed SQLite commits plus finalized dirty follow-up onto current `origin/main`.
  Reference: `git rev-list --left-right --count origin/main...HEAD` from the SQLite worktree.
  Acceptance: Worktree is clean after rebase, SQLite parity fixtures pass, and focused cargo checks for `perry-stdlib --features database-sqlite`, runtime/codegen/api-manifest, and manifest consistency pass.
