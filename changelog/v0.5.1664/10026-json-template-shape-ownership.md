### Fixed

- Preserve the reusable JSON object template's shape descriptor across full
  garbage collections, including when the ordinary parse-shape cache no longer
  owns it. Repeated parsing can then safely create a fresh object from the
  retained template. Regression coverage verifies descriptor ownership with
  the redundant cache removed and checks template relocation during evacuation.

This fragment backfills the release note omitted from #10026. Version 0.5.1527
records that already-landed fix for the next release.
