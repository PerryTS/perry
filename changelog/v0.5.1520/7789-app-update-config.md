### Added

**An application Perry compiles can now carry its own update check.** Perry's
CLI has checked for updates for a long time; an app it *builds* could not, so
shipping one meant the author wrote a version check by hand or shipped none and
hoped users noticed.

```json
{ "perry": { "update": {
    "source": "npm", "package": "myapp", "command": "self-update"
} } }
```

A `perry.update` block — or an `[update]` table in perry.toml, which wins key by
key — is validated at compile time and baked into the executable. It splits into
two halves, and which is which matters:

- **Perry does the noticing.** At startup the app reads its own state file and
  prints a two-line notice on stderr when the last lookup found something newer.
  The author writes no code for this.
- **The app does the asking.** The version lookup uses the app's own `fetch()`,
  from a few lines the author adds. Perry supplies everything else: whether a
  lookup is due, which URL and headers the configured source needs, and how to
  read the answer.

That division is the one `docs/src/updater/overview.md` already states for the
desktop updater — *"Download lives in TS (using existing `fetch()`) — Rust only
handles the security-critical and platform-touching pieces"* — and it applies
with more force here, because `perry-runtime` links into every compiled binary.
An HTTP stack added for this would be paid for by every program that never
checks for an update.

**With no block configured, nothing is emitted.** Not an empty blob, and not a
disabled one: a binary that configures no updates is byte-identical to one built
before this existed. A feature whose off-state still emits code is one you cannot
prove is off.

<details>
<summary><b>Validation is a build failure, on purpose</b></summary>

A warning scrolls past in build output; the consequence lands on the app's users,
who get no notices and no error — the feature does nothing and nobody can tell
why. So these are errors:

- **a URL must be `https://`**, with loopback allowed for local testing. Plain
  HTTP is refused because an on-path attacker can answer "you are current" and
  suppress an update. The loopback exemption stops at a host boundary, so
  `http://localhost.example.test` is refused like any other remote host;
- **each source needs the keys it reads** — `url` for `gh-releases` and
  `custom`, `package` for the npm-shaped ones;
- **a zero check interval is rejected**, since it would ask on every run;
- **an app with no version** has nothing to compare against.

`enabled = false` keeps the settings on disk and emits nothing, rather than
embedding a disabled block complete with its URL and startup call.

The version is taken from perry.toml's `[project] version` when present, so it
agrees with what the rest of the binary reports — otherwise a dual-manifest
project could compare against a number the app never claims to be.
</details>

<details>
<summary><b>The four sources, and what each refuses to do</b></summary>

`gh-releases` and `custom` read the configured URL. The npm-shaped pair request
the *abbreviated* packument — smaller, cacheable, and the document npm itself
asks for.

The public registry is asked **without credentials**; a token there is a leak,
not a convenience. GitHub Packages produces **no URL at all** without a token,
rather than an anonymous request whose 404 reads as "up to date" — which would
have the app report itself current forever. It also gets no npmjs.com link,
since that package may be private or absent there and the notice would show a
URL that 404s.

Each shape reads only its own fields, so a registry answering a `gh-releases`
request is an error rather than a version of `""`. The npm `latest` tag is read
from inside `dist-tags`, not the document root, because a packument carries
version strings in several places.

Recording a lookup carries the notice state across, so a refresh cannot reset the
notify throttle.
</details>

<details>
<summary><b>Lessons taken from the CLI's own review rather than rediscovered</b></summary>

Perry's CLI update surface was reviewed after merging, and three findings apply
verbatim to this half. They are already handled here:

- the notify interval is keyed to the announced **version**, not the clock alone,
  so an interval set to stop nagging about one release cannot hide the release
  that fixed it;
- the state file is written to a per-write temporary name, because two instances
  of the same app can run at once and one shared name lets each rename a file the
  other is still writing;
- an unreadable timestamp notifies rather than staying silent, since silence on
  one bad write would hide updates indefinitely.

Four more are specific to an app rather than a CLI. The notice is **stderr
only** — an app's stdout belongs to the app. Control characters are stripped,
because a release name is attacker-influenceable terminal input and a notice must
not repaint somebody's screen. Startup reads its argument list with `args_os`,
since `args()` panic-drops the process on non-UTF-8 input and this runs before
any app code. And an unparseable version never reads as newer: node-smol's
equivalent compared against a hardcoded `"0.0.0"`, which made every release look
newer than the running binary.
</details>

<details>
<summary><b>Two gaps closed on the way past</b></summary>

The blob is part of the **object-cache fingerprint**. Without it, adding
`perry.update` and rebuilding incrementally would serve the cached entry object
from before — shipping a binary with no update check while the build reported
success. Same class as the `dbgloc` and `fmath` entries that file already
documents.

`PERRY_UPDATER_TABLE`'s own comment says it is "auto-derivable from" the
api-manifest entries, but those are hand-listed and **nothing checked that the
two agreed**. A dispatch row without its entry makes the strict
unimplemented-API gate reject user code that calls it, in somebody else's build.
There is now a parity test, plus one asserting no runtime symbol starts with a
prefix Windows synthesizes no-op stubs for — a stubbed symbol returns garbage
rather than failing to link.
</details>

<details>
<summary><b>Tests</b></summary>

**53.** Ten on the compiler side cover the parse and every validation rule,
including the loopback host boundary and that perry.toml overrides package.json
key by key while leaving keys it does not set alone. Forty-one in the runtime
cover the blob reader, every gate, the per-platform state directory, version
comparison, the throttle, control-character stripping, all four request shapes
and response parsers, and each shape rejecting the others' documents. Two more
assert dispatch/manifest parity.

Several exist because a test found the bug: an app with no config reported "go
ahead" instead of "not configured"; and values containing a quote or backslash
were read back with their escapes intact, doubling on every save until a URL was
unusable. Both are sabotage-verified — reverting either fix turns its test red.

Beyond the units there is a **wiring test** driving the whole startup path —
blob in, notice out, state advanced, second run quiet — because everything else
here is a piece tested in isolation, and a feature whose pieces all pass while
the path between them is broken is what ships doing nothing.

**Verified end to end:** a configured project's binary contains the blob, the
same project without the block produces one that does not, the configured binary
runs normally, and a plain-HTTP URL fails the build with a message naming the
key.
</details>

### Documentation

New page `docs/src/cli/app-updates.md`, written for the app author: the smallest
configuration that does something, every key in both spellings, how to choose a
source, the mistakes that fail the build and why each is an error, the cases
where a user's run will not check at all, where the state file lives per
platform, how to give users an off switch, and the few lines that perform the
lookup.
