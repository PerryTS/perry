`perry run <dir>` now resolves the directory to its project entry instead of passing the directory itself into module collection.

An explicit directory input is treated as a project root: its `perry.toml` entry is read relative to that directory, falling back to `<dir>/src/main.ts` then `<dir>/main.ts`. Previously `perry run .` read `perry.toml` from the current working directory and handed the directory straight to module collection, which fails on Windows.

Fixes #8908.
