### Fixed

- **Async `unlink` no longer overtakes an earlier async write to the same
  path.** Perry parks `fs.writeFile` / `fs.appendFile` and their promise forms
  on a later event-loop turn so an immediate `process.exit()` can abandon them
  like Node does. The unlink entry points still performed their filesystem
  operation synchronously, however, so a cleanup issued before that turn saw
  `ENOENT`; the parked write then ran and recreated the file.

  Same-path unlinks now join the existing deferred-operation queue whenever a
  parked write is present. Unlinks with no preceding write, writes through a
  file descriptor, and operations on unrelated paths retain their existing
  behavior.

  This fixes Claude Code under `--bare -p`: its graceful-shutdown cleanup now
  removes `~/.claude/sessions/<pid>.json` instead of leaving a Perry-only file
  whose PID, session ID, cwd, and timestamp vary on every run.
