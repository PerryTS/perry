### Changed

- **No `perry-codegen` integration suite is held out of per-PR CI any more.**
  #8264 recorded eight assertions as baseline reds and parked their suites in
  `scripts/ci_e2e_scope.py`'s `SUITE_EXCLUSIONS`. #8302 fixed the six
  `native_proof_buffer_views` ones (un-excluded in #8321) and #8333 fixed the
  remaining two; this removes their entries and maps both suites, leaving
  `SUITE_EXCLUSIONS` empty.

  A stale exclusion is not a red build — it is silence. The suite keeps being
  skipped, so coverage a fix earned back never runs, which is #7708's failure
  mode. Un-excluding also requires mapping the suite: `--self-test` refuses a
  suite that appears in neither list, and that is what catches the half-done
  version of this change.
