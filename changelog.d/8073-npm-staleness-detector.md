### Added

- **CI now notices when npm has fallen behind `main`.** In
  [#7491](https://github.com/PerryTS/perry/issues/7491) a user compiling with the
  npm build hit `unresolved external symbol
  js_ext_http_client_request_is_handle` at link time. The fix had been on `main`
  for weeks; the version npm was serving as `latest` was **a month old**. Every
  gate in the repo was green throughout, and every one of them was right — they
  all measure `main`, and `main` is not what `npm install @perryts/perry` gives
  you. Nothing compared the two, so the only detector was a user reading the
  versions tab and filing a bug.

  `scripts/check_npm_publish_freshness.py` is that comparison. It reads the full
  packument (the abbreviated `application/vnd.npm.install-v1+json` document omits
  the `time` map, which is where the age lives) for every package published from
  `npm/`, and measures it against `[workspace.package] version`:

  | signal | budget | why |
  |---|---|---|
  | age of the published `latest` | 14 days | Counted **only while the tree is ahead** of it. A month-old publish with nothing unreleased behind it is a quiet week; a month-old publish with 290 merged patches behind it is #7491. This is the signal that would have caught it on day 15. |
  | patch distance | 500 | Every merge to `main` bumps the workspace patch, so the distance is a commit count wearing a version number. A backstop for a cadence spike inside an unexpired age budget — deliberately *not* tuned tight enough to enforce a release cadence, because a budget that goes red every week gets muted. |
  | platform packages match the launcher | no budget | `npm/perry/package.json.tmpl` pins its optionalDependencies to its own exact version, so a platform package left behind by a partial publish breaks installs outright while both halves sit comfortably inside their own budgets. |

  Run against the live registry the day it was written, it reproduces the report
  exactly: `latest` is `0.5.1220`, published 41 days ago, 290 patch bumps behind
  a tree at `0.5.1510` — and it turned up one thing the issue did not.
  **`@perryts/perry-win32-arm64` returns HTTP 404: the package has never been
  published at all**, although `stage-npm.sh` stages it, the release matrix
  builds `aarch64-pc-windows-msvc`, and the launcher pins it as an
  optionalDependency. That is a maintainer call, not something an outside
  contributor can fix, so the check simply names it.

  **Wired as `.github/workflows/npm-publish-freshness.yml`** — daily cron, plus a
  path-filtered pull-request arm that runs only the offline halves. It is
  deliberately **not** a required status context: whether a release has been cut
  is not something a PR author can fix, and blocking every PR on the maintainer's
  publishing state is how a gate gets muted. It fails loudly on the Actions page
  and maintains one sticky issue, updated in place, closed automatically once the
  registry catches up — the same shape `gate-freshness.yml` uses. It is also
  registered in `scripts/gate_freshness.json`, so if this cron itself goes dark
  the freshness sweep says so.

  Written against CLAUDE.md's four ways a gate can be unable to fail:

  - **An unreachable or unparseable registry is RED, not a skip.** Three attempts,
    then a red run naming the transport error. This detector exists because a
    silence read as health for a month; a network failure that exits 0 is that
    same silence wearing a CI badge. The cost of that choice is a re-run after a
    registry blip; the cost of the other choice is #7491 again.
  - **`time.modified` is never a fallback for a missing per-version timestamp.**
    It moves on any metadata change — a deprecation, a dist-tag edit, an
    ownership change — so a package nobody has published to in a month can carry
    a `modified` stamped this morning. Reading it would make the stalest possible
    package look freshly cut. No timestamp for the published version is red.
  - **The manifest cannot silently shrink.** `scripts/npm_publish_freshness.json`
    must name exactly the set of packages `npm/*/package.json.tmpl` declares; a
    new platform target that is published but not watched fails the check,
    offline, on the pull request that adds it.
  - **A scoped run is a diagnostic, not a verdict.** `--package` and `--packument`
    (replay a saved registry response) refuse to touch the sticky issue, so a
    subset can never declare the whole set healthy.
  - **`--self-test` is sabotage, not exercise.** It plants twelve packuments one
    at a time — the #7491 shape, age-only, distance-only, the fresh-`modified`
    trap, a missing dist-tag, a dist-tag pointing at an unlisted version, a
    prerelease on `latest`, npm ahead of the tree, an x.y line change, and both
    budget boundaries — plus a partial publish that is inside every budget, an
    unreachable registry, and manifest coverage drift, and asserts the verdict
    for each. It also asserts that a stale verdict *prints* `STALE` and *exits*
    non-zero, since a red verdict with exit 0 is a gate that cannot fail. Both
    halves were mutation-checked: reverting the unreachable-registry decision to
    a quiet pass, and adding a `time.modified` fallback, each turn the self-test
    red.

  Docs: `docs/src/contributing/releasing.md` §4a. No compiler or runtime change.
