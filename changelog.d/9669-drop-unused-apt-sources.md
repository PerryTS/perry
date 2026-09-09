- **`apt-get update` no longer gates CI jobs on third-party mirrors we never
  install from.** GitHub's Ubuntu images ship Chrome and Microsoft sources, and
  `apt-get update` exits non-zero if **any** source fails. On 2026-09-09
  `dl.google.com`'s chrome-stable index served a `Hash Sum mismatch` and
  reddened the release tier three times running — 22 jobs in run 34383689667,
  then `Install clang` and `Install mysql client` in runs 34384580491 and
  34386043331. None of those jobs want Chrome.

  Every `apt-get update` in `test.yml` (7 sites), the two in
  `release-packages.yml`'s `build` job, and `setup-llvm22` now do two things:

  1. **Drop the unused sources by CONTENT, not filename.** The first attempt
     removed `google-chrome.list` and changed nothing, because image
     `ubuntu24/20260907.300` had moved these to deb822 `.sources` files. The
     removal now greps `/etc/apt/sources.list.d/` for the host.
  2. **Let the install be the gate.** `apt-get update`'s exit status aggregates
     sources we depend on with sources we do not, so it cannot answer the
     question we actually care about. The update is now advisory and the
     `apt-get install` that follows decides — the same "verify by reaching for
     what you came for" shape that already kept `setup-llvm22` green through all
     three outages while its neighbours failed.

  Two things worth recording, because each cost a five-hour tier. The repo
  returned **200 from a developer machine** while runners kept failing, so "it
  has cleared" was wrong twice — a third-party mirror's health has to be judged
  from where the job runs. And matching by name rather than by cause failed here
  for the third time in this workstream, after an apt pin glob missed
  `libllvm22` and a `KNOWN_FAIL` name list missed a renamed test.
