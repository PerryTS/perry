### Fixed

**The `[update]` section of `~/.perry/config.toml` was deleted every time
anything else saved that file.** `update_checker` read `[update] server`
through its own private structs, but `PerryConfig` — the struct `save_config`
writes — had no field for it, and serde rebuilds the file from the struct. So
answering the telemetry prompt, the compatibility-report prompt or the beta
notice, or running a setup wizard, silently discarded the user's update
settings.

`PerryConfig` now owns the section, and `update_checker`'s private duplicate
reader is gone, so there is one loader rather than two views of the same file.

### Added

**An `[update]` section, so update behaviour is a setting rather than an
all-or-nothing environment variable.** Before this, the only control was
`PERRY_NO_UPDATE_CHECK`; there was no way to check less often, and no way to
say anything once instead of in every shell.

```toml
[update]
mode = "notify"            # off | notify | prompt | auto
check_interval_hours = 24  # how often to ask what the latest version is
notify_interval_hours = 0  # 0 = mention it every run, which is what Perry did
prompt_default = false     # what Enter means in prompt mode
```

`off` and `notify` are the two behaviours that already existed, and `notify` is
the default, so a user who never opens the config sees no change. `prompt` and
`auto` are accepted and documented here but not yet wired to an install — that
is the next slice, deliberately separate, because replacing the binary a user
is running deserves its own review.

`PERRY_UPDATE_MODE` sets the same thing for one run.

<details>
<summary><b>Precedence, and the two rules that outrank everything</b></summary>

Strongest first:

1. `PERRY_NO_UPDATE_CHECK`, and now also `NO_UPDATE_NOTIFIER` — the de-facto
   ecosystem spelling (npm's `update-notifier`, with `GH_NO_UPDATE_NOTIFIER`
   and `DENO_NO_UPDATE_CHECK` by analogy). Someone who sets either has already
   told every tool on their machine what they want, and no config file may
   argue: these beat `PERRY_UPDATE_MODE=auto` and a configured `auto` alike.
2. `CI`, by presence rather than by an exact `"true"`/`"1"` match, since CI
   systems are not consistent about the value. An exported-but-empty `CI=` is
   still *not* CI, matching `is-ci`'s truthiness test.
3. A non-terminal stderr, or `--format` asking for machine-readable output.
   Nobody is reading a notice in either case, and interleaving one into
   parseable output is the classic update-notifier bug report.
4. `PERRY_UPDATE_MODE`, then the config file, then `notify`.

An unparseable `PERRY_UPDATE_MODE` falls through to the config rather than
selecting something: `of` must not quietly mean `off`, and must certainly not
mean `auto`.
</details>

<details>
<summary><b>Two ways a config file could lose data, both closed</b></summary>

An unrecognized `mode` deserializes to a known-unknown rather than failing.
That matters more than it looks: `load_config` parses the whole file as one
document and falls back to defaults on any error, so a rejected `mode` would
have discarded the user's license key and API token with it — and the next
save would have written that loss to disk. A typo now costs one warning line
and the default mode.

Unrecognized *keys* inside `[update]` are preserved across a load/save round
trip, so a key written by a newer Perry — or by hand, ahead of a feature
landing — is not dropped by an older one. That is the same defect as the
erasure above, one level down.
</details>

<details>
<summary><b>Two correctness details in the checker itself</b></summary>

The cache is now written to a temporary file and renamed over the target,
rather than truncated in place. Two `perry` processes can be running at once —
one finishing a background check while another records a notice — and a reader
arriving mid-write got a partial file, which `load_cache` discards entirely.
The rename goes through the existing `replace_path` helper because a plain
`fs::rename` onto an existing file fails on Windows, which would have made
every write after the first silently do nothing.

`fetch_latest_version` rebuilds the cache struct from scratch, so it now
carries the last-notified timestamp across a refresh. Without that,
`notify_interval_hours` would reset every check interval and quietly stop
working.
</details>

**Tests.** 13 new, all in the required per-pull-request job: the precedence
table including that the kill switches beat everything and that an empty `CI`
is not CI; that an unknown mode leaves the rest of the file intact; that
unknown keys survive a round trip; the throttle arithmetic including that an
unreadable timestamp notifies rather than staying silent forever; and the
erasure regression itself. Verified by sabotage — marking the new field
`#[serde(skip)]` turns the erasure test red.

`cargo test -p perry`: 902 passed, 0 failed.
