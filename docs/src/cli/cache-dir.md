# Cache directory

Where Perry writes its on-disk caches for a project. Defaults to
`<project-root>/node_modules/.cache/perry`, the find-cache-dir convention
used by babel-loader, eslint, and most of the JS toolchain.

Everything Perry caches lands directly under this directory: per-module
object files (`objects/<target>/`), the build-cache manifest (`build/`),
the link-cache manifest (`link/`), the behavioral SBOM (`audit.json`),
sandbox-exec profiles (`buildrs-<pkg>.sandbox`), and HIR miss dumps
(`debug/`) when `PERRY_CACHE_DEBUG_HIR=1`.

## Four ways to set it

Precedence, highest to lowest: CLI flag, then `PERRY_CACHE_DIR`, then
`perry.toml`, then `package.json`. So `perry.toml` overrides `package.json`,
and the env var and CLI flag override `perry.toml`:

```
# 1. Per-build CLI flag (wins over everything)
perry compile --cache-dir /var/cache/perry myapp.ts

# 2. Per-shell environment
PERRY_CACHE_DIR=/var/cache/perry perry compile myapp.ts

# 3. Per-project perry.toml, alongside the other [perry] settings
[perry]
cacheDir = "/var/cache/perry"

# 4. Per-project package.json (most common)
{
  "perry": {
    "cacheDir": ".perry-cache"
  }
}
```

A relative path resolves against the project root, so two projects that
both set `cacheDir = ".cache"` keep separate caches. An absolute path is
used as-is.

## Notes

- The directory is created automatically on the first build. If it can't
  be created (read-only root, permission error), Perry silently degrades
  to a no-op cache for that run rather than failing the build.
- The cache directory must be writable for caching to take effect.
- The default `node_modules/.cache/perry` rides along with the
  already-ignored `node_modules/`, so nothing new lands in `git status`.
- Older Perry versions wrote to `<project-root>/.perry-cache/`. That
  directory is now stale and can be deleted; Perry no longer reads or
  writes it. Run `perry cache clean` to wipe the current cache.
- An explicit `--cache-dir` is useful for CI caches (point it at a path
  your CI restores between runs), shared build farms (a fast local SSD
  instead of an NFS-mounted project root), and read-only project roots
  (relocate the cache to a writable scratch dir).

The object cache is machine-local — it bakes in `-mcpu=native` codegen —
so sharing a cache directory across machines with different CPUs (rsync,
NFS, a Docker bind-mount) can produce `SIGILL` at runtime. Scope a shared
`--cache-dir` to a single CPU family.
