### Fixed

- **The build cache no longer hands back a binary built by a different
  compiler.** The `perry_build_id` check re-fingerprinted the path *recorded in
  the manifest* and compared it to the recorded value — which asks "is the
  binary I recorded still unchanged?", and is trivially true whenever a
  different `perry` runs the second build. The recorded binary is sitting
  exactly where it was, so the check passed, the cache reported
  `"hit": true, "reason": "manifest-match"`, and the build was skipped
  entirely: no relink, output file untouched, nothing printed, exit 0.

  It now compares against the compiler running now, via the same
  `current_perry_fingerprint()` used when the manifest is written.

  `perry_version` did not cover this. During pass development the version
  rarely moves between rebuilds, which is the reason `perry_build_id` exists
  at all (#544) — this restores the guarantee that issue was closed on.

  How it surfaced: a `.ts` probe compiled by a pre-fix compiler kept its stale
  executable when recompiled by a fixed one, so a genuine fix read as not
  working. The phantom was then bisected onto an unrelated commit before the
  cache was suspected. Touching the source does not help, because sources are
  verified by sha256 rather than mtime; only a different output path or a
  cleared cache does.
