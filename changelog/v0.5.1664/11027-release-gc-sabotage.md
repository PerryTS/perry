### Tests

The remembered-set coverage sabotage witness now runs under release unit tests,
so the dirty-scan remembering arm can no longer regress behind a silently
ignored debug-only assertion.
