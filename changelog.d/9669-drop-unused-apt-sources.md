- **Unused third-party apt sources are removed before every `apt-get update` in
  `test.yml`.** GitHub's Ubuntu images ship lists we never install from, and
  `apt-get update` exits non-zero if **any** source fails. On 2026-09-09
  `dl.google.com`'s chrome-stable repo served a `Hash Sum mismatch` and reddened
  the tier twice: 22 jobs in run 34383689667, then `Install clang` in
  `gc-stress-build`, `native-abi-evidence-packet` and `compiler-output-regression`
  plus `Install mysql client` in `drizzle-mysql-smoke` in run 34384580491. None
  of those want Chrome.

  All seven `apt-get update` sites now drop `google-chrome.list` and
  `microsoft-prod.list` first. `setup-llvm22` additionally retries and, if the
  update still reports errors, checks whether `llvm-22-dev` actually resolves —
  verifying by reaching for what it came for rather than trusting an aggregate
  exit status that mixes our source with ones we do not care about.

  Worth recording how long this took to see: the repo returned **200 from a
  developer machine** while runners kept failing, so "it has cleared" was wrong
  twice. A third-party mirror's health has to be judged from where the job runs.
