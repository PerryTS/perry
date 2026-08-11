# Updates

Perry checks whether a newer version exists, in the background, and mentions it
on stderr at the end of a run. That is all it does by default. Everything below
is optional.

## Doing nothing

The default is to check at most once a day and print one line when there is
something newer. Perry never installs anything unless you ask it to.

Checks are skipped entirely when any of these is true:

- `PERRY_NO_UPDATE_CHECK` is set to `1` or `true`
- `NO_UPDATE_NOTIFIER` is set — the same variable npm's `update-notifier`
  reads, so setting it once covers every tool that honours it
- `CI` is set to anything other than an empty or explicitly-false value
- stderr is not a terminal, so nobody would see the notice
- `--format` asks for machine-readable output, where a notice would land in the
  middle of what you are parsing

## Configuring it

Everything lives in an `[update]` section of `~/.perry/config.toml`.

```toml
[update]
mode = "notify"
check_interval_hours = 24
notify_interval_hours = 0
```

| key | default | what it does |
|---|---|---|
| `mode` | `notify` | How much Perry does. See below. |
| `check_interval_hours` | `24` | How often to ask what the latest version is. |
| `notify_interval_hours` | `0` | Minimum gap between two notices about the same version. `0` means every run. |
| `prompt_default` | `false` | Which answer Enter picks in `prompt` mode. |
| `min_age_hours` | `24` for `auto`, `0` otherwise | How long a release must have existed before `auto` installs it. |
| `skip_version` | unset | A version to stay quiet about. Usually written by answering the prompt. |
| `source` | unset | Where to ask. See [Choosing a source](#choosing-a-source). |
| `package`, `registry` | Perry's own | For the npm-shaped sources. |
| `server` | unset | A mirror to prefer, and the URL for `source = "custom"`. |

### The four modes

| mode | behaviour |
|---|---|
| `off` | Never check, never say anything. |
| `notify` | Check in the background, print one line when something is newer. **The default.** |
| `prompt` | Notify, then ask whether to install. |
| `auto` | Install at the end of a successful run, without asking. |

`perry update --mode auto` writes the setting for you.

`prompt` and `auto` both refuse in three situations, and say why:

- **The command you ran failed.** You are reading an error; a question about
  upgrading is noise, and an unattended install would bury it. Perry falls back
  to a plain notice.
- **A package manager owns this Perry.** Homebrew, npm, apt and winget each
  track what they installed. Overwriting the binary underneath leaves that
  record wrong, so Perry names that manager's own command instead.
- **The install directory is not writable.** Checked before anything is
  downloaded, so you get one sentence naming `sudo perry update` rather than a
  download that dies at the last step. Perry never escalates on its own.

`auto` additionally waits out `min_age_hours` — see [The cooldown](#the-cooldown).

## Choosing a source

By default Perry asks its release infrastructure. If you installed through npm,
it asks npm instead, because that is the version your package manager can
actually install.

| `source` | what it reads |
|---|---|
| `gh-releases` | The GitHub releases API. |
| `npm` | An npm registry's `latest` dist-tag. Public registry unless `registry` says otherwise. |
| `gh-registry` | GitHub Packages. Needs `GH_TOKEN` or `GITHUB_TOKEN`. |
| `custom` | Any HTTPS URL in `server` returning `{"version": "..."}`. |

```toml
[update]
source = "npm"
package = "@perryts/perry"
```

A source that fails is reported rather than quietly retried somewhere else. If
you said "ask npm", a failure means npm did not answer — not that Perry should
go and ask GitHub.

### One thing a source can never do

A source answers *what is the latest version*. It never decides where a binary
comes from. Downloads and their signature always come from the release
infrastructure.

That is deliberate. The signature is what makes a self-update safe to run, and
`source` is a URL you can point anywhere — so if it could redirect the
download, this setting would be a way to install arbitrary code.

## The cooldown

`auto` will not install a release that is younger than `min_age_hours`, which
defaults to a day.

A release that was published by mistake, or pulled shortly after, or published
by someone who should not have been able to, is most dangerous in its first
hours. Waiting a day costs nothing and means your machine is not the one that
finds out. `notify` and `prompt` are unaffected — they tell a human, who can
decide.

If a source does not report a publish date, the release counts as **too fresh**
rather than old enough. The abbreviated npm document has no dates, so treating
unknown as "old enough" would switch the cooldown off for exactly the people
using the cheapest source. Set `min_age_hours = 0` to turn it off deliberately.

## Skipping one version

In `prompt` mode the third answer is "skip this version and stop asking about
it". That is not the same as turning notices off: the skipped version goes
quiet, and the next release is mentioned normally — so the release that fixes
whatever made you skip does not stay hidden too.

## What a check sends

The request itself, and nothing else: a user agent naming Perry's version, and
the platform's artifact name when asking the release infrastructure. No
identifiers, and no relationship to telemetry — `PERRY_NO_TELEMETRY` does not
affect update checks, and these settings do not affect telemetry.

## Where things are kept

| path | what |
|---|---|
| `~/.perry/config.toml` | The `[update]` section. |
| `~/.perry/update-check.json` | The last check's answer, and when you were last told. |

Both are safe to delete; Perry rebuilds them.

## Environment variables

| variable | effect |
|---|---|
| `PERRY_NO_UPDATE_CHECK=1` | Switch the whole surface off. Beats every config setting. |
| `NO_UPDATE_NOTIFIER` | The same, using the ecosystem-wide spelling. |
| `PERRY_UPDATE_MODE` | `off`/`notify`/`prompt`/`auto` for one run. Beats the config file, loses to the two above. |
| `PERRY_UPDATE_SERVER` | Prefer this release URL. Highest priority for downloads. |
| `GH_TOKEN`, `GITHUB_TOKEN` | Used only by `source = "gh-registry"`. |
