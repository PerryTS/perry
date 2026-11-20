### Added

**Where Perry asks "what is the latest version?" is now a choice.** It used to
walk one fixed list — an override, the config, Perry Hub, then the GitHub
releases API — and read a GitHub-releases-shaped document from whichever
answered first. That is fine while everyone installs the same way, and wrong as
soon as they do not: an npm user's "latest" is whatever the registry's `latest`
dist-tag says, and asking GitHub instead can announce a version their package
manager cannot install yet.

```toml
[update]
source = "npm"              # gh-releases | npm | gh-registry | custom
package = "@perryts/perry"  # npm-shaped sources; defaults to Perry's own
registry = "..."            # npm-shaped sources; defaults to the public registry
server = "..."              # the URL for `custom`, and the mirror override
```

Unset keeps the historical ladder, so nothing changes for anyone who does not
set it — except on an **npm-managed install**, which now defaults to asking npm,
because that is the version its own package manager can actually install.

<details>
<summary><b>The split that matters: checking is not downloading</b></summary>

A check source answers one question and returns a version, a link, a publish
time and a headline. It does **not** decide where the binary comes from.
Artifacts and their signed manifest always resolve from the release
infrastructure, whatever the check source is.

That separation is load-bearing rather than tidy. The manifest — Ed25519 over
the artifact's digest and version — is what makes a self-update trustworthy,
and a check source is a URL a user can point anywhere. Letting it redirect the
download would turn a configuration setting into a way to install an arbitrary
binary. Whoever answers "what is new?" never gets to answer "what should I
run?", and there is a test that fails if a source ever leaks into the artifact
ladder.

The old `get_update_servers` and its private config reader are **deleted**
rather than left beside the new code, so the compiler enforces that both call
sites moved. A new abstraction with the old ladder still wired up underneath is
the shape where four sources exist, pass their own tests, and are never
reached.
</details>

<details>
<summary><b>Credentials go to exactly one of the four</b></summary>

The npm shapes ask for the *abbreviated* packument
(`Accept: application/vnd.npm.install-v1+json`) — smaller, cacheable, and the
document npm itself requests for this question. It also avoids GitHub's
unauthenticated API rate limit, which the old ladder shared with everything
else on the machine.

The public registry is asked **without credentials**, and a test asserts no
`Authorization` header is sent: a token there would be a leak, not a
convenience. GitHub Packages does need one, so that shape reads `GH_TOKEN` /
`GITHUB_TOKEN` and fails with a sentence naming the fix when neither is set,
rather than retrying anonymously and reporting the resulting 404 as "up to
date".

A configured source does not fall back to the ladder when it errors. Somebody
who said "ask npm" and got a failure wants to hear that, not a version from
somewhere they never named.
</details>

<details>
<summary><b>Tests</b></summary>

11 new, all parsing real response shapes from string fixtures so no network is
involved:

- a GitHub release document, including that the `v` prefix is stripped;
- an abbreviated packument, which has no `time` map — so the publish date reads
  "unknown" rather than being invented, which matters because the release
  cooldown in the next slice depends on it;
- a full packument, which does supply it;
- a custom manifest with only a `version`, and one with every optional field;
- that each shape **rejects the others' documents** rather than reading a field
  that happens to be present — a registry answering a gh-releases request must
  be an error, not a version of `""`;
- that a scoped package's `/` is percent-encoded, or the registry reads the
  scope as a path segment and answers 404;
- that an unknown `source` name falls back instead of failing, so a config
  written by a newer Perry does not break an older one;
- that `custom` with no URL is treated as a missing key rather than a default;
- that an npm install defaults to npm and every other channel keeps the ladder;
- that no check source can reach the artifact ladder;
- and both credential rules.

`cargo test -p perry`: 925 passed, 0 failed.
</details>
