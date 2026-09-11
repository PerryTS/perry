- **`apt-get update` no longer gates CI jobs on third-party mirrors we never
  install from.** GitHub's Ubuntu images ship Chrome and Microsoft apt sources,
  and `apt-get update` exits non-zero if **any** source fails. On 2026-09-09
  `dl.google.com`'s chrome-stable index served a `Hash Sum mismatch` and
  reddened the release tier three times running — 22 jobs in run 34383689667,
  then `Install clang` and `Install mysql client` in runs 34384580491 and
  34386043331. None of those jobs want Chrome.

  Ten sites now do two things: the 7 `apt-get update`s in `test.yml`, the 2 in
  `release-packages.yml`'s `build` job (the release critical path), and
  `setup-llvm22`.

  1. **Drop the unused sources by CONTENT, not filename.** The first attempt
     removed `google-chrome.list` and changed nothing, because image
     `ubuntu24/20260907.300` has moved these to deb822 `.sources` files. The log
     shows the `rm` running and Chrome being fetched 0.2 s later.
  2. **Let the install be the gate.** `apt-get update`'s exit status aggregates
     sources we depend on with sources we do not, so it cannot answer the
     question we care about. The update is advisory; the `apt-get install` that
     follows decides. `setup-llvm22` previously depended on
     `sudo apt-get update -qq`; this change introduces its package-resolution
     fallback when the update fails.

  The removal is written as an `if` rather than `... || true`, because
  `scripts/gc_gate_wiring_check.py` rightly rejects `|| true` inside the
  `gc-stress` jobs and cannot distinguish a benign swallow from a real one. An
  `if` CONDITION is exempt from `set -e`, so the guard is safe under `-e` and
  `pipefail` while suppressing nothing. Verified under both, including the
  no-match and missing-directory cases.

  Two things worth recording, because each cost a five-hour tier. The Chrome
  repo returned **200 from a developer machine** while runners kept failing, so
  "it has cleared" was wrong twice — a third-party mirror's health has to be
  judged from where the job runs. And matching by name rather than by cause
  failed here for the third time in this workstream, after an apt pin glob
  missed `libllvm22` and a `KNOWN_FAIL` name list missed a renamed test.
