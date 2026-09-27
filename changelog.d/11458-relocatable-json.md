Resolve compiled CommonJS modules from the embedded path registry when their
build-host files no longer exist. This lets static JSON requires, including
mime-db used by Axios, run after a binary is deployed without its source tree.

Keep filesystem/symlink resolution for existing files, and use normalized
absolute registry keys for missing file, extension, and directory-index
candidates. Presence checks do not initialize modules and retain registered
paths for undefined exports and failed initializers. Add resolver regressions
and an integration test that deletes all sources before exercising JSON loads,
cache eviction/reload, createRequire, and primitive JSON values.
