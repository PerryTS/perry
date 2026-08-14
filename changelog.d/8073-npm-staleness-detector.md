### Added

- **A daily advisory check now reports when npm packages lag behind `main`
  (#7491).** It compares every package published from `npm/` with the workspace
  version, enforcing an unpublished-age budget, a patch-distance backstop, and
  exact launcher/platform-package version agreement. Pull requests run only
  offline validation with read permissions; scheduled and main-branch manual
  runs maintain one sticky issue. Registry failures, malformed timestamps,
  missing packages, and manifest coverage drift fail closed.
