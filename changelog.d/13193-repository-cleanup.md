### Changed

- Replace the long `CLAUDE.md` with a lean `AGENTS.md` that holds only per-PR obligations, build and verification traps, silent failures and gate policy. `CLAUDE.md` now imports it, and the doc-claim and Node-version lints read `AGENTS.md`.
- Keep the version only in `Cargo.toml`. The release preflight and the npm publish gate no longer compare it with a `CLAUDE.md` line, so PRs no longer edit `CLAUDE.md`.
- Remove stale root files (committed logs, task prompts, agent session notes) and obsolete issue records from `docs/`. Group the remaining point-in-time evidence under `docs/audits/` with an index.
- Regenerate the documentation translation catalogs with the pinned toolchain.
