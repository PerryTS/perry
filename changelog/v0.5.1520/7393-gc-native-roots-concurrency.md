**Fixed** `gc-native-roots.yml` had no `concurrency` group, so three of its four
platform arms had never executed.

Nothing superseded a stale run, and the workflow's four-arm matrix multiplied
across every push. Ten consecutive runs were checked: the `macos-14` arm was
`queued` in **every one of them** — never executed, not once. `ubuntu-latest`
(x86-64 ELF) and `windows-latest` (PE) likewise. Only the aarch64 arm ever
reached a runner, which is why it was the only arm ever observed red or green.

Three quarters of this matrix has been reporting nothing while presenting as
four-platform coverage — CLAUDE.md's fourth hazard in a different guise. It also
made #7392 unanswerable: whether that segfault is ELF-specific cannot be
distinguished from "the macOS arm has never run the probe."

`cancel-in-progress: false` alone would not fix it. GitHub allows at most one
PENDING run per group and cancels the previously pending one when a new run
enters, regardless of that setting (#7205). Keying push runs on the SHA gives
every merged commit a group of its own while PR runs supersede freely — the same
shape `llvm-inprocess.yml` already uses (#7357).
