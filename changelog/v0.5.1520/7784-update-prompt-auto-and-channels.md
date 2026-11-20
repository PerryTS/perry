### Added

**`prompt` and `auto` update modes now do something, and refuse to do the wrong
thing.** The previous slice made the modes configurable; this wires them to the
existing signed self-updater, behind three refusals.

**A package-managed install is never replaced in place.** `perry update`
overwrites the running executable, which is right for a tarball or `install.sh`
install and wrong for every managed one: Homebrew, npm, apt and winget each keep
their own record of what is installed and at what version, and overwriting the
file underneath leaves that record lying. `prompt` and `auto` now detect the
owner and name that owner's command instead:

| owner | what Perry says to run |
|---|---|
| Homebrew | `brew upgrade perryts/perry/perry` |
| npm | `npm install -g @perryts/perry@latest` |
| apt | `sudo apt update && sudo apt install --only-upgrade perry` |
| winget | `winget upgrade PerryTS.Perry` |

npm gets an extra sentence, because it is the worst case: Perry ships as a
wrapper package plus a per-platform binary package, so replacing the binary also
desyncs it from the wrapper that launched it.

**Nothing is offered after a command that failed.** The user is looking at an
error; a question about upgrading is noise at the worst possible moment, and an
unattended install would bury the error under progress output. Both active modes
fall back to a plain notice.

**An unwritable install directory is reported, not attempted.** `install.sh`
targets `/usr/local/bin`, which is root-owned on a default macOS and most Linux
boxes. That is now checked *before* anything is downloaded, so the outcome is
one sentence naming `sudo perry update` rather than a half-finished install. Perry
never escalates on its own.

**`perry update --mode <off|notify|prompt|auto>`** saves the setting and exits,
so the one thing people are most likely to change does not require hand-editing
TOML. It is a read-modify-write through the shared loader, so the rest of the
file comes back out the way it went in.

**`perry doctor`** now reports the effective mode and, when there is one, the
package manager that owns the binary — the two questions behind "why did it not
update".

<details>
<summary><b>Why the channel detection fails open</b></summary>

Every rule answers "is this definitely managed?", never "is this definitely
unmanaged?", and an unrecognised layout resolves to self-managed.

That asymmetry is deliberate. Guessing "managed" wrongly would refuse to
self-update a plain tarball install — the majority case, and the one with no
other upgrade path. Guessing "self-managed" wrongly costs an in-place update on
a machine that had a package manager available, which is recoverable by running
that manager.

The paths are canonicalized before classification, because Homebrew's `perry` in
`/usr/local/bin` is a symlink into the Cellar; classifying the link rather than
its target would miss every Homebrew install there is.

apt requires **both** a dpkg file list and a dpkg-owned path, because dpkg does
not own `/usr/local` — that is `install.sh`'s directory. The path alone would
misclassify a hand-placed binary; the dpkg list alone would claim a tarball
install on a machine that also has the `.deb` installed somewhere else. The check
is a file-existence test rather than a `dpkg -S` subprocess, since this runs on
the update path of every command.
</details>

<details>
<summary><b>Prompting needs stdin, not just stderr</b></summary>

The mode gate already requires stderr to be a terminal. That is not enough to
ask a question: stdin can be a pipe while stderr is a tty, and reading from it
would either block the command or take whatever the pipe happened to contain as
consent. `prompt` degrades to a plain notice when stdin is not a terminal.

`auto` asks nothing, so it does not need stdin — but it does still require the
command to have succeeded, an unmanaged install, and a writable directory.
</details>

<details>
<summary><b>Tests</b></summary>

24 new, all in the required per-pull-request job. The decision is a pure
function of the mode plus four facts about the machine, so every refusal is
asserted directly rather than left inside an `if` in the middle of a teardown
path:

- both active modes downgrade to a notice after a failed command;
- both refuse on all four managed channels, and name a command for each;
- both report elevation rather than attempting an unwritable install;
- `prompt` degrades without stdin while `auto` does not need it.

The channel table covers Homebrew under all three prefixes, npm for global, nvm
and project-local layouts, apt with and without each half of its rule, both
winget delivery shapes, and four unrecognised layouts that must fail open.
Classification splits on both path separators rather than using
`Path::components`, so the winget cases run on every host instead of only on
Windows.

Verified end to end: writing `mode` into a real config file that already had a
`license_key` and an unknown `[update] future_key` left both intact.

`cargo test -p perry`: 914 passed, 0 failed.
</details>
