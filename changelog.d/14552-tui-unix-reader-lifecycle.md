### Fixed
- Cancel and join the Unix TUI input reader on normal shutdown, release stdin for later consumers, and allow a fresh reader after stopping or reaching EOF without changing Windows reader behavior.
