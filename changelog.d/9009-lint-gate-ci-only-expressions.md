`scripts/run_lint_gates.sh` no longer reports a permanent false failure.

The extractor read each `run:` block line by line, so a backslash-continued
gate was emitted as its first line only and then executed that way. #8928's
step ran as `python3 scripts/ci_cargo_test_shard.py --package perry \`, with
`--total-shards` and `--validate` stranded on the continuation lines, and
failed on a missing argument for everyone on every run — `1 of 58 FAILED`.

Backslash continuations are now joined before matching, and a command carrying
a GitHub Actions expression is skipped: it is substituted in CI and never
locally, so running it passes an empty value and fails for reasons unrelated to
the tree. Skipped gates are named in the output and subtracted from the total
rather than dropped — a gate that quietly disappears is worse than one that is
permanently red, which is the subject of this script.

The join leaves the derived list otherwise identical (56 commands, the two
ratchet steps still extracted — their `git cat-file … || git fetch …` prelude
is a separate logical command from the gate below it). Sabotage checks both
ways: an expression injected into an unrelated gate becomes a second named
skip, and a genuinely broken gate still reports `FAIL` and exits 1.

The expression is built by concatenation, not written literally, because this
heredoc sits inside a process substitution and bash 3.2 (macOS) treats a
literal `${{` in the body as an unterminated parameter expansion — which kills
the script before it runs anything, and would do so only on macOS.
