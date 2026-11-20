- **The release's tarball check now derives its expected set from the publish
  manifest instead of a hardcoded `9`.** v0.5.1519 failed to publish after a
  fully green 14-leg build (run 34438751300) with
  `Expected 9 exact npm tarballs; found 7`.

  Nothing was wrong with the build. When the musl legs were dropped pending
  #9382, `PLATFORM_PACKAGES` in `scripts/publish/constants.mts` was correctly
  trimmed to 6, so `ALL_PACKAGES` is 6 platforms + 1 wrapper = 7.
  `prepare-ci-packages.mts` packed 7 and **passed its own check against
  `ALL_PACKAGES`** — and then `release-packages.yml`'s separate hardcoded `9`
  rejected the same set one step later. Two expressions of one fact, and only
  one of them was updated. The publish step never ran, so nothing reached the
  registry, no tag was cut, and the version was not burned.

  The check now reads the manifest that the previous step generates from
  `ALL_PACKAGES`, and matches **by name** rather than by count — a count cannot
  say *which* package is missing, which is the only question worth asking when
  this fires. A missing tarball now reports
  `manifest package(s) have no packed tarball: perryts-perry-linux-arm64-…tgz`.
  Count equality is still asserted so a stray extra tarball fails too, and a
  manifest of fewer than two packages is refused outright.

  Written with `while read` rather than `mapfile` so it runs on bash 3.2 and can
  be exercised on a developer machine, not only on a runner. It was tested
  against five cases before shipping — all present; one platform missing; a
  stray extra; a too-small manifest; and the real-world shape with stale empty
  `npm/perry-linux-*-musl` directories still on disk. A step that has already
  failed one release does not deserve to be shipped untested a second time.

  Note for follow-up: the doc comments in `constants.mts` still say "The 8
  platform packages" and "All 9" above 6- and 7-element arrays. They are stale
  in exactly the way that caused this, but correcting them touches a
  non-plumbing path, so they are left for a normal PR rather than a release pin.
