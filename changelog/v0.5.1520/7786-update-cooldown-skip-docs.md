### Added

**`auto` waits a day before installing a brand-new release.** `min_age_hours`
defaults to 24 for `auto` and 0 for every other mode, so a notice still mentions
a release the moment it exists while an unattended install holds off.

A release published by mistake, pulled shortly after, or published by someone
who should not have been able to is most dangerous in its first hours. Waiting
costs nothing and means your machine is not the one that finds out. `notify` and
`prompt` are unaffected — they tell a human, who can decide.

**An unknown publish date counts as too fresh, not as old enough.** The
abbreviated npm document carries no dates, so treating unknown as "old enough"
would switch the cooldown off for exactly the people using the cheapest source —
a protection present in the config and absent in effect. `min_age_hours = 0`
turns it off deliberately.

**The prompt has three answers.** "No" and "never tell me about this one" are
different intentions, and with only two answers a user who does not want one
specific release has to switch the whole mode off — which then hides the release
that fixes it. Answering the third writes `skip_version`, which suppresses
exactly that version; the next release is mentioned normally.

**The notice says what the release is,** not only that one exists. Sources that
carry a title now pass it through, and it prints under the version line at no
extra request.

### Documentation

New page `docs/src/cli/updates.md`, covering the default behaviour, everything
in `[update]`, the four modes and their three refusals, the four check sources,
the cooldown, skipping a version, exactly what a check transmits, and where the
two files live. `perry update`'s section in `commands.md` is rewritten around
`--mode`; `installation.md` gains the per-package-manager upgrade table; the
environment tables in `flags.md` gain `NO_UPDATE_NOTIFIER` and
`PERRY_UPDATE_MODE`.

<details>
<summary><b>Tests</b></summary>

Four new, bringing the update surface to 22 in `update_policy`:

- `auto` holds off inside the cooldown and installs past it, while `notify` and
  `prompt` are never held back;
- an unknown release age is treated as too fresh, and an explicit `0` still lets
  it through so nobody is stuck;
- the cooldown defaults to a day for `auto` only, and an explicit value wins for
  every mode;
- a skipped version suppresses itself and **not** the next one, which is what
  separates it from switching notices off.

`cargo test -p perry`: 929 passed, 0 failed.
</details>
