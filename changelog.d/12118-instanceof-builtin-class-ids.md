`instanceof` no longer mints a class function object for a builtin class id
such as `PassThrough`; it falls back to the declared chain walk. A regex
program cell no longer copies stack bytes when it is initialised.
