# Update checks in apps you build

An app Perry compiles can tell its own users when a newer version exists. You
configure it once and Perry bakes the settings into the executable.

Two halves, and it is worth knowing which is which:

- **Perry does the noticing.** At startup your app reads its own state file and
  prints a notice if the last lookup found something newer. You write no code
  for this.
- **Your app does the asking.** The version lookup uses your app's own
  `fetch()`, from a few lines you add — see
  [Performing the check yourself](#performing-the-check-yourself). Until you add
  them, nothing is ever recorded and the notice never appears.

This is off unless you ask for it. A project with no `perry.update` block
produces a binary identical to one built before this feature existed.

## The smallest useful configuration

```json
{
  "name": "myapp",
  "version": "1.2.3",
  "perry": {
    "update": {
      "source": "npm",
      "package": "myapp",
      "command": "self-update"
    }
  }
}
```

That is enough for the configuration side. Once you add the lookup call, a run
that finds a newer `myapp` records it, and the next run prints two lines to
stderr:

```
Update available: myapp 1.2.3 → 1.4.0
  Run `myapp self-update` to update
```

If you have not implemented a `self-update` command, leave `command` out and the
second line points at the release page instead. Perry will not tell your users
to run something that does not exist.

## Every setting

`perry.update` in package.json, or an `[update]` table in perry.toml using
`snake_case` names. perry.toml wins key by key, so a project can keep defaults
in package.json and override them per build.

| package.json | perry.toml | default | what it does |
|---|---|---|---|
| `source` | `source` | — | Required. `gh-releases`, `npm`, `gh-registry` or `custom`. |
| `url` | `url` | — | Required for `gh-releases` and `custom`. |
| `package` | `package` | — | Required for `npm` and `gh-registry`. |
| `registry` | `registry` | public npm | Registry base URL for the npm-shaped sources. |
| `tag` | `tag` | — | Release-tag pattern for `gh-releases`. |
| `command` | `command` | none | The command your notice suggests. Omit if you have none. |
| `checkInterval` | `check_interval_hours` | `24` | Hours between lookups. |
| `notifyInterval` | `notify_interval_hours` | `24` | Minimum hours between two notices about the same version. |
| `binName` | `bin_name` | output name | What to call the app in its own notice. |
| `appId` | `app_id` | `binName` | Names the state directory. |
| `skipEnv` | `skip_env` | none | An environment variable that switches the check off. |
| `enabled` | `enabled` | `true` | `false` keeps the settings and emits nothing. |

The version comes from `[project] version` in perry.toml when you have one, and
from package.json's `version` otherwise — the same value the rest of your binary
reports, so the notice cannot compare against a number your app never claims.

### Choosing a source

| `source` | reads | needs |
|---|---|---|
| `gh-releases` | The GitHub releases API | `url`, optionally `tag` |
| `npm` | A registry's `latest` dist-tag | `package` |
| `gh-registry` | GitHub Packages | `package`; pass a token to `embeddedCheckUrl` |
| `custom` | Any HTTPS URL returning `{"version": "..."}` | `url` |

## Mistakes are caught at build time

These fail the build rather than warning. A warning scrolls past in build
output; the consequence lands on your users, who get no notices and no error —
the feature simply does nothing and nobody can tell why:

- **A URL must be `https://`.** Plain HTTP is refused. An attacker on the
  network can answer "you are current" and suppress an update, and a warning in
  build output is not where that gets noticed. `http://localhost` and the
  loopback addresses are allowed so you can test against a local server.
- **A source must have the keys it reads** — `url` for `gh-releases` and
  `custom`, `package` for the npm-shaped ones.
- **`checkInterval` cannot be 0.** That would ask on every run. To disable
  checks, remove the block or set `enabled: false`.
- **Your app needs a version.** Set `version` in package.json, or
  `currentVersion` in the block.

## When your app will not check

Your users get a check only when all of these hold. None of it is configurable
by you, because each one is a case where a notice does harm:

- their stderr is a terminal — otherwise the notice lands in whatever is reading
  your app's output;
- `CI` and `CONTINUOUS_INTEGRATION` are unset;
- `PERRY_NO_UPDATE_CHECK` and `NO_UPDATE_NOTIFIER` are unset, or set to `0`,
  `false`, `off`, `no` or the empty string. Any other value disables the check,
  including one this list does not name — somebody who wrote `=please` is asking
  not to be checked. `NO_UPDATE_NOTIFIER` is the variable npm's
  `update-notifier` reads, so a user who set it once has already told every tool
  on their machine;
- your own `skipEnv` variable, if you named one, is unset;
- the command being run is not your `command`. An `app self-update` invocation
  does not check on its way to updating.

Your app's **stdout is never touched**. The notice is stderr only.

## Where the state lives

One file per app, in the platform's cache directory:

| platform | path |
|---|---|
| macOS | `~/Library/Caches/<appId>/update-check.json` |
| Linux | `$XDG_CACHE_HOME/<appId>/update-check.json`, or `~/.cache/<appId>/update-check.json` |
| Windows | `%LOCALAPPDATA%\<appId>\update-check.json` |

It records when the last check happened, what it found, and when the user was
last told. Deleting it is safe. Two apps never share one, so your app's notice
cannot silence another's.

The `notifyInterval` throttle is keyed to the *version*, not just the clock. If
you set a week to stop nagging about `1.4.0`, and `1.4.1` ships the next day
fixing something, your users still hear about it.

## Turning it off for a build

```json
{ "perry": { "update": { "enabled": false, "source": "npm", "package": "myapp" } } }
```

Nothing is embedded — not a disabled block. The settings stay in the file for
when you want them back.

## Giving your users an off switch

Name one and Perry honours it:

```json
{ "perry": { "update": { "source": "npm", "package": "myapp",
                          "skipEnv": "MYAPP_NO_UPDATE_CHECK" } } }
```

Document it in your own README. The global variables above work regardless.

## Performing the check yourself

The startup notice reports what a *previous* run recorded. To make a run actually
ask, call the check from your own code — Perry gives you everything except the
request itself, which uses your app's own `fetch()`:

```typescript
import {
  embeddedCheckHeaders,
  embeddedCheckUrl,
  embeddedRefreshDue,
  recordEmbeddedResponse,
} from "perry/updater";

async function checkForUpdates(): Promise<void> {
  if (!embeddedRefreshDue()) return;

  const url = embeddedCheckUrl(process.env.GH_TOKEN);
  if (!url) return; // nothing should be requested — see below

  const headers: Record<string, string> = {};
  for (const line of embeddedCheckHeaders().split("\n").filter(Boolean)) {
    const at = line.indexOf(": ");
    headers[line.slice(0, at)] = line.slice(at + 2);
  }

  const response = await fetch(url, { headers });
  if (response.ok) recordEmbeddedResponse(await response.text());
}
```

Call it wherever a slow operation is already acceptable — after your work, not
before it. The next run prints the notice.

### Why the request is yours

Perry keeps the parts that must agree with the settings it compiled — which URL,
which headers, and how to read each of the four source shapes — and leaves the
network call to you. An HTTP stack added to the runtime for this would be paid
for by every program that never checks for an update.

It also means the check obeys your app's own proxy configuration, timeouts and
error handling, rather than a second set hidden inside the runtime.

### An empty URL is an answer

`embeddedCheckUrl()` returns `""` when no request should be made. Respect it.

The case that matters is `gh-registry` with no token available: that request
would 404, and a 404 reads as "no newer version", so your app would report itself
up to date forever. Perry declines to give you a URL rather than let that happen.

### Recording is validated

`recordEmbeddedResponse` reads the body according to your configured `source`, so
a registry answering a `gh-releases` request is rejected rather than read as
version `""`. A version that does not parse is also rejected — one malformed
answer would otherwise become a permanent "update available" your users cannot
dismiss.

It returns 1 when something was recorded, 0 otherwise. There is no need to act on
that; the next startup either has something to say or does not.
