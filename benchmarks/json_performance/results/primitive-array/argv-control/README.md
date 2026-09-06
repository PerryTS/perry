# Executable-name length control

The full A/B harness names the candidate `worker` and the reference
`baseline-worker`. To test whether this length difference explains the
numeric-parse RSS regression, identical copies of each binary were run under
both six- and fifteen-character names, with seven rotated fresh-process trials
per combination. Each process parses `numbers_1m.json` 96 times after two
warmups, with default GC. Binary hashes are recorded in every raw trial.

| Binary | Name length | Median CPU/call | Median peak RSS |
|---|---:|---:|---:|
| Unicode reference | 6 | 1.576 ms | 64.531 MiB |
| Unicode reference | 15 | 1.577 ms | 64.531 MiB |
| Primitive-array candidate | 6 | 1.579 ms | 66.781 MiB |
| Primitive-array candidate | 15 | 1.577 ms | 66.781 MiB |

The name-length hypothesis is rejected: the RSS difference follows the binary
and persists under both lengths. This does not identify its underlying cause.
The harness naming scheme was left unchanged. The quiet gate passed; see
`window.json`, `summary.json`, `trials.jsonl` and `reproduce.py`.
