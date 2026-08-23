### Fixed

- Complete the `node:test` parity tracker: test and suite registration is now
  deferred until module evaluation finishes, hooks respect their suite scope,
  async and callback tests settle before reporting, subtests aggregate into one
  Node-compatible summary, runtime directives and plans affect results, local
  mocks restore after each test, `run()` returns a populated test stream, and
  snapshot serializers validate every entry.
