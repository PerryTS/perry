Fixed the `zizmor` GitHub Actions security gate, which had been failing on every
`main` commit since #7353.

`#7353` created `.github/actions/setup-llvm22/action.yml` and, with it, four
high-severity findings. The gate went red on that commit and stayed red for
roughly 40 consecutive `main` commits. `#7388` (clang-22 in setup-llvm22) and
`#7393` (a `concurrency` group for gc-native-roots) are both innocent — they
only shifted the reported line numbers, which is what made them look
implicated.

One finding is properly fixed. The Windows arm interpolated a composite-action
input straight into a PowerShell script body (`$ver = "${{ inputs.version }}"`),
which `template-injection` flags at **High** confidence: a template expansion is
substituted as raw text before pwsh parses the line, so an input carrying a
quote plus a statement separator would execute as code with the runner's
privileges. The input now arrives through an `env:` block and is read as
`$env:LLVM_VERSION`, which is a plain string load with no such step.

Three findings are suppressed, with reasoning, in `.github/zizmor.yml`. All
three are `github-env` at **Low** confidence, and all three are the single
`LLVM_SYS_221_PREFIX=<prefix>` line the action exists to write, once per
platform arm. The audit was measured to be satisfiable only by not writing the
environment file at all — a dynamic value into `$GITHUB_ENV` always reports (a
static literal does not), and the pwsh arm reports even for a literal because
zizmor cannot evaluate `Out-File`. The clean alternative is `$GITHUB_OUTPUT`
plus composite-action outputs, and it was measured across `.github/workflows`
at **44 jobs** needing an `id:` and **140 downstream steps** each needing
`env: LLVM_SYS_221_PREFIX: ${{ steps.<id>.outputs.prefix }}`, because llvm-sys
reads the prefix from the environment at build time. That recreates precisely
the "44 inline recipes" duplication the action was written to remove, across 18
files, none of it verifiable outside CI — so it was rejected as a bad trade for
a Low-confidence finding on filesystem paths produced by locally installed
toolchains, in an action whose callers only ever run under `pull_request` and
`push`. The carve-out is a dated ratchet with an explicit delete-condition, in
the same style as the file's existing entries.

Verified with the repo's SRI-pinned zizmor 1.28.0: with the pristine config and
the template-injection fix the audit reports 3 high findings and exits 14; with
the carve-out it exits 0, and the `ignored` count rises from 119 to 122 — three,
matching the three suppressed findings exactly, so the entry is not
over-broad.
