# turnloop gtk4 — taking tokio out of `perry-ui-gtk4`

Branch `turnloop/gtk4-detokio`, based on `turnloop/integration` at `ce480bb208`
(every lane through P11 — there is no p10 report — plus `main` through
v0.5.1580). Built and tested on the shared Linux box (EPYC 9354P, gtk4 4.14.5 /
shumate 1.2.beta / webkitgtk-6.0 2.52.6 / gstreamer 1.24.2). Nothing here runs
on macOS or Windows, nothing was benchmarked, and the one thing this change
cannot be proved against — a real desktop session with a StatusNotifierItem
host — is named in "What is not verified".

## The blocker was not real

P8's inventory recorded this edge with:

> `ksni` and `mpris-server` REQUIRE tokio (they are zbus clients with a `tokio`
> feature). Removing it means replacing both crates or dropping tray/MPRIS
> support on Linux.

Both halves are wrong, and the two crates' own manifests say so:

| | what the manifest says |
|---|---|
| `ksni` 0.3.6 | `default = ["tokio"]`, **and** `async-io = ["dep:async-io", "dep:async-lock", "dep:async-executor", "dep:futures-lite", "dep:futures-channel", "dep:task-local", "zbus/async-io"]`. `src/compat.rs` is a two-backend shim with a `compile_error!` if both are on — they are peers, not a default and a hack. |
| `mpris-server` 0.10.0 | `tokio = ["zbus/tokio"]`, and `default` is empty. It depends on `zbus 5.14` with default features, which are `["async-io", "blocking-api"]`. |
| `zbus` 5.16.0 | `default = ["async-io", "blocking-api"]`; `tokio` is one opt-in backend of several. |

Perry asked for `features = ["tokio"]` on both, and then carried
`tokio = { version = "1", features = ["rt","sync","time","net","io-util"] }` to
feed them. The whole surface was three uses in two files.

What actually drives the two services with tokio out of the picture, read from
the crates rather than assumed:

* **`ksni` in `async-io` mode carries its own executor.**
  `compat::spawn` queues onto a process-global `async_executor::Executor` and
  kicks a driver thread that lives as long as there is a task
  (`ExecutorState::kick_driver`). `service::run` then builds its zbus
  connection with `internal_executor(false)` and spawns the connection's tick
  loop onto that same executor, explicitly *before* it registers with the
  watcher. Nothing outside ksni has to supply a runtime, and nothing has to
  keep polling `spawn()`'s future after it resolves.
* **zbus without its `tokio` feature drives its own connection.**
  `connection::builder::start_internal_executor` — which is
  `#[cfg(not(feature = "tokio"))]` — spawns a `zbus::Connection executor`
  thread per connection. `mpris-server` does not turn the internal executor
  off, so the MPRIS connection is self-driving. (Under the `tokio` feature that
  function does not exist: zbus spawns onto the ambient runtime instead, which
  is exactly why the old code needed one.)
* **`async-io` runs a reactor thread on demand** (`async_io::driver`), so a
  plain `block_on` on any thread makes progress on socket readiness.

## What changed

`crates/perry-ui-gtk4/Cargo.toml`: `tokio` deleted; `mpris-server` loses its
`tokio` feature; `ksni` becomes `default-features = false, features =
["async-io"]`; `async-executor`, `futures-lite` and `async-channel` are added,
all three already in the graph under ksni / mpris-server / zbus.

`crates/perry-ui-gtk4/src/background.rs` (new, 102 lines, two thirds of them the
explanation above) is the whole replacement runtime:

* `block_on` — `futures_lite::future::block_on`, for the two one-shot
  handshakes that used `Runtime::block_on`.
* `spawn_detached` — one `async_executor::Executor` on one thread named
  `perry-ui-async`, started lazily on first use and parked in `Executor::run`
  thereafter, for what used `Runtime::spawn`.

The module's doc comment is the long form of the table above, so the next
person to ask "why is there no tokio here" reads it from the code.

`spawn_detached` catches a panicking task rather than letting it unwind out of
`Executor::run`. That is not decoration: an `async_executor` runner that unwinds
is gone for the process lifetime, and every later `spawn_detached` would queue
behind a runner that no longer exists — a tray that silently stops updating, in
a crate no CI job builds. tokio contained a panicking task to that task, and so
does this.

### Where each piece of work runs, before and after

This is the part that mattered more than the diff, so it is stated per call
site. **Nothing moved onto the GTK main thread, and nothing that was on the GTK
main thread moved off it.**

| work | before | after |
|---|---|---|
| `tray::create` — SNI registration handshake | blocks the calling (GTK main) thread in `Runtime::block_on` | blocks the calling (GTK main) thread in `futures_lite::future::block_on` |
| the KSNI service loop (D-Bus method calls, `activate`, menu callbacks) | a `perry-tray` tokio worker | ksni's own `async-io` driver thread |
| `set_icon` / `set_tooltip` / `attach_menu` → `refresh()` | `Runtime::spawn`, fire-and-forget on the tokio worker | `background::spawn_detached`, fire-and-forget on `perry-ui-async` |
| `tray::destroy` → `Handle::shutdown` | ditto | ditto |
| JS tray callbacks | marshalled to the GTK main loop with `glib::MainContext::invoke` | unchanged |
| `mpris::run_server` (update drain + `properties_changed`) | `perry-mpris` thread, `Runtime::block_on` on a current-thread runtime | `perry-mpris` thread, `futures_lite::future::block_on` |
| the MPRIS zbus connection's socket | the same `perry-mpris` thread (tokio tasks on its current-thread runtime) | a `zbus::Connection executor` thread zbus starts itself |
| main thread → MPRIS property pushes | `tokio::sync::mpsc::unbounded_channel` | `async-channel` unbounded, `try_send` |
| D-Bus method calls → GStreamer | `std::sync::mpsc` drained by `poll_tick` on the GLib thread | unchanged |

`refresh()` deliberately keeps a thread rather than using glib's
`MainContext::spawn_local`, which would have cost nothing: a tray can be
configured before `app.run()`, and a future spawned on a main context that is
not turning yet would sit undelivered. The old behaviour was "applies
immediately, main loop or not", and that is preserved.

**Thread-count effect, stated because it is a real cost:** a program using both
the tray and MPRIS goes from roughly 2–3 threads (one `perry-tray` tokio worker,
the `perry-mpris` thread, and tokio's blocking pool on demand) to about 6:
`perry-ui-async`, ksni's own driver, `async-io`'s reactor, `perry-mpris`, one
`zbus::Connection executor` for the MPRIS connection (ksni's connection needs
none — `internal_executor(false)`), and the `blocking` crate's pool in place of
tokio's. All are idle-parked. A program that uses neither feature starts none of
them, before or after.

### What did not move

Nothing in `perry-ui-gtk4`: after this change the crate has no tokio-family
manifest edge of any kind, no `tokio::` path in its source, and no tokio in its
subtree of `Cargo.lock`. The tray and MPRIS features both survive in full — no
capability was dropped and no crate was replaced.

The other 38 edges are outside this lane. Eleven workspace crates still have a
direct `tokio` edge — `perry-container-compose`, `perry-ext-fastify`,
`perry-ext-http`, `perry-ext-ioredis`, `perry-ext-mongodb`, `perry-ext-mysql2`,
`perry-ext-net`, `perry-ext-nodemailer`, `perry-ext-pg`, `perry-ext-ws` and
`perry-stdlib` — and every one of them is blocked on something in groups A–L of
P8's plan, not on a mistaken reading of a manifest. This lane's finding does not
generalise to them: they hand tokio futures to a tokio runtime, which is a real
dependency, where `ksni` and `mpris-server` merely *offered* a tokio backend
that Perry opted into.

## GC decisions

One JS value lives behind these services: `TrayState.on_click`, a NaN-boxed
closure. It is scanned by `scan_gtk4_tray_gc_roots`, which walks the `TRAYS`
registry — and that is untouched here. The futures this change moves onto a
different executor hold a `ksni::Handle`, whose path to that `f64` runs through
the same `Arc<Mutex<TrayState>>` the registry holds, so the value stays
reachable to the scanner exactly as before. `destroy()` still takes the handle
out of the registry before spawning the shutdown task, which is what the old
code did; nothing reads `on_click` after that point.

No raw heap pointer is cached anywhere new, so nothing needed
`gc_register_mutable_root_scanner`.
`scripts/gc_runtime_root_holders.py` sees all three new/changed statics —
`background::EXECUTOR` (`Executor<'static>`), `background::STARTED`
(`OnceLock<bool>`) and `mpris::UPDATE_TX`, whose type changed from
`tokio::sync::mpsc::UnboundedSender<Update>` to `async_channel::Sender<Update>`
— and classifies none of them as able to hold a heap pointer, which is correct:
`Update` carries a `Metadata` (strings) or a `PlaybackStatus`. The gate is
green, and it was checked that it actually parsed the new file rather than
skipping it.

## The inventory

`python3 scripts/tokio_inventory.py` — **38 manifest edges**, down from 39,
across 12 workspace crates. Group **M** is gone; `--list` now prints 38 edges in
12 groups with no M row. `scripts/tokio_inventory.json` is updated in the same
commit (the gate fails on a stale entry as well as a new one).

The `blocker` text is not merely deleted. `docs/turnloop/p8-report.md` carries
the correction in the two places the wrong claim was written down — the
per-crate summary table and the group-M row of the costed plan — saying what was
actually true rather than quietly dropping the row. P8's total of 46 planned
edge-removals is left as P8's arithmetic: it is already stale for group J, whose
edges a later lane removed without renumbering it.

`source_sites` for `perry-ui-gtk4` goes 7 → 6, and the crate now appears in
`--list`'s "tokio-shaped source but NO manifest edge" section. That is the
documented false-positive case: the script's regex counts `block_on`, and all
six remaining hits are one — three in `background.rs`, one each in `tray.rs` and
`media_playback.rs` calling it, and the pre-existing `main_context.block_on` in
`clipboard.rs`. None has anything to do with tokio. The number is explicitly
ungated; it was not gamed by renaming the helper.

### Lockfile

`tokio` does not leave `Cargo.lock` — eleven other crates still reach it — so
the gate's `lockfile` map is unchanged. Two second-order effects are worth
naming:

* **`zbus` loses its `tokio` dependency entirely**, which is the proof that no
  other workspace crate was quietly holding `zbus/tokio` on. Only `ksni` and
  `mpris-server` depend on zbus, and only `perry-ui-gtk4` depends on those two.
* **`tokio` loses its `tracing` feature for the whole workspace** — zbus was its
  only enabler (`tokio/full`, which perry-stdlib and the root manifest use, does
  not include `tracing`). Nothing in the tree consumes tokio's instrumentation:
  there is no `console_subscriber` or `tokio-console` anywhere. This is a
  strictly smaller tokio for every crate that still links one, and it is the
  only way this change reaches code outside `perry-ui-gtk4`.
* One new transitive crate: `task-local` 0.1.1, MIT OR Apache-2.0 (both already
  in `deny.toml`'s allow list), pulled by ksni's `async-io` feature.

## Test evidence

### The build, which is the acceptance test

This crate is **never compiled by PR CI**. The exclusion is maintained in
`workspace-architecture.json` and consumed by `ci_test_scope.py`, with the
reason spelled out in `test.yml`: "needs system pango/gtk via pkg-config; runner
image doesn't have libgtk-4-dev installed by default". Its doc-tests are
disabled too (`test.yml` v0.5.873). The only workflow that builds it is
`release-packages.yml`, with `cargo build --profile dist --target <t> -p
perry-ui-gtk4`. A mistake here surfaces at release time, not in review, so the
build was run for real rather than `cargo check`ed — on the base commit first, so that a failure afterwards could
only be this change's.

```
# base ce480bb208, before any edit
cargo build --release -p perry-ui-gtk4     -> Finished in 2m55s, 37 warnings
# this branch
cargo build --release -p perry-ui-gtk4     -> Finished in 2m49s, 37 warnings
cargo clippy --release -p perry-ui-gtk4 --all-targets  -> exit 0
```

(The final numbers above are from the last rebuild of the branch as pushed; the
clippy run was re-done after a `touch` of `background.rs` to confirm the
`Checking perry-ui-gtk4` line appears rather than a cached replay.)

Warning count is identical (37, all pre-existing: `js_string_from_bytes` /
`js_closure_call2` extern redeclarations, dead fields). Clippy reports 106
warnings for the lib, none of them in `background.rs`, `tray.rs` or
`media_playback.rs` — the only clippy hit in a file this change touches is the
pre-existing `js_closure_call2` redeclaration at `media_playback.rs:34`.

The box needed one package the brief's list had missed:
`libgstreamer-plugins-base1.0-dev` (for `gstreamer-app-1.0.pc`). It is installed
now. `libshumate`'s pkg-config name is `shumate-1.0`, not `libshumate-1.0`.

### The headless D-Bus test, which is the evidence that it *works*

"It compiles" says nothing about a tray. Every way of getting an executor wrong
here — a future nobody polls, a zbus that panics looking for a reactor, a
fire-and-forget update dropped on the floor — compiles fine and produces a tray
that never appears. So the change ships with
`crates/perry-ui-gtk4/tests/dbus_services_without_tokio.rs`, which talks to a
real session bus:

```
$ dbus-run-session -- cargo test --release -p perry-ui-gtk4 \
      --test dbus_services_without_tokio -- --nocapture

running 1 test
tray: org.kde.StatusNotifierItem-901945-1 is live, Id = perry-tray-901945-1
tray: NewToolTip observed — background::spawn_detached is driving futures
mpris: org.mpris.MediaPlayer2.perry-901945 is live
mpris: PropertiesChanged carried the pushed title
threads: ["async-io", "blocking-1", "dbus_services_w", "perry-mpris",
          "perry-ui-async", "tray_and_mpris_", "tray_and_mpris_",
          "zbus::Connectio", "zbus::Connectio"]
test tray_and_mpris_serve_a_real_bus_without_tokio ... ok

test result: ok. 1 passed; 0 failed
```

(The two `tray_and_mpris_` threads are unnamed `std::thread::spawn`s, which
inherit the spawning thread's `comm` on Linux: ksni's own executor driver and
one of the test's signal watchers. `blocking-1` is the `blocking` crate's pool,
which zbus uses for blocking calls. The names that matter are the ones that are
**not** there.)

It asserts four things a live bus can show and a unit test cannot:

1. **The tray registers.** `org.kde.StatusNotifierItem-<pid>-1` appears in
   `ListNames`, and a `GetAll` on `/StatusNotifierItem` answers with
   `Id = perry-tray-<pid>-1` — so the ksni service loop and its zbus connection
   are both running, on nobody's runtime.
2. **A tray update actually round-trips.** After `set_tooltip`, the test waits
   for the `NewToolTip` signal. This is the assertion that matters: the SNI
   properties are computed on demand from `TrayState`, so re-reading one would
   pass whether or not `refresh()` ever ran. Only the signal proves
   `background::spawn_detached` → `Handle::update` → the service loop → D-Bus
   happened. A `spawn_detached` that silently dropped its future — the exact
   failure mode of getting the executor wrong — fails this.
3. **MPRIS comes up and pushes.** `set_now_playing` brings
   `org.mpris.MediaPlayer2.perry-<pid>` onto the bus, and the pushed title
   arrives in a `PropertiesChanged` on `/org/mpris/MediaPlayer2` — so the
   `async-channel` hop, the `perry-mpris` thread's `block_on` drain, and zbus's
   self-started connection thread all work.
4. **None of it is tokio.** The test reads `/proc/self/task/*/comm` and fails if
   any thread name starts with `tokio-`, and requires `perry-ui-async` and
   `perry-mpris` to be present. A counter that stays at zero is a finding, not a
   pass — this is the equivalent for a change whose subject is a *missing*
   dependency.

Without `DBUS_SESSION_BUS_ADDRESS` the test prints `SKIPPED (not a pass)` and
returns, so `cargo test -p perry-ui-gtk4` stays green on a bus-less box. That
skip path was run deliberately as a control in the same session, so the green
result above cannot be a silent skip:

```
$ env -u DBUS_SESSION_BUS_ADDRESS cargo test --release -p perry-ui-gtk4 \
      --test dbus_services_without_tokio -- --nocapture
SKIPPED (not a pass): no DBUS_SESSION_BUS_ADDRESS. Re-run as
  dbus-run-session -- cargo test --release -p perry-ui-gtk4 --test dbus_services_without_tokio -- --nocapture
```

### The tested tree is the pushed tree

Verified rather than assumed, because the box tree was kept in sync by `rsync`
rather than by `git`: every build-relevant file in `/root/claude-turnloop-gtk4`
was compared by `md5sum` against `git show <pushed sha>:<path>` after the push.
`Cargo.toml`, `lib.rs`, `tray.rs`, `media_playback.rs`, `background.rs`, the
test, and `Cargo.lock` all match byte-for-byte, so the build, clippy and D-Bus
results above are results for the commit on the branch.

### Local gates

Run from the branch, all green:

| gate | result |
|---|---|
| `python3 scripts/tokio_inventory.py` | `38 manifest edges across 12 workspace crates, 20 tokio-family packages in Cargo.lock — unchanged` |
| `python3 scripts/tokio_inventory.py --self-test` | `OK (7 planted changes, all caught)` |
| `cargo fmt --all -- --check` | clean |
| `./scripts/check_file_size.sh` | `no Rust source files exceed 2000 lines` |
| `python3 scripts/addr_class_inventory.py` | `passed (1604 files scanned)` |
| `python3 scripts/gc_runtime_root_holders.py` | `OK — 1485 holder declarations scanned` |
| `git diff --name-only HEAD~1 \| python3 scripts/ci_e2e_scope.py` | selects nothing — see below |
| `cargo test --release -p perry-ui-gtk4 --lib` (on the box) | 1 passed, 0 failed |
| `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` (on the box) | `Finished` in 4m45s, exit 0 |
| `cargo deny check licenses bans sources` (on the box) | `bans ok, licenses ok, sources ok` |

The `ci_e2e_scope.py` line is worth its own sentence: the new integration test
**cannot** turn a PR red. That script shares its exclusion list with
`ci_test_scope`, which reads `linux_host_excluded_members` from
`workspace-architecture.json` — and `perry-ui-gtk4` is in it — so the diff
selects no suite. That is also why the test has to be run by hand, and why this
report spells out the command.

### What was NOT run, and why

* **No gap-suite A/B.** `perry-ui-gtk4` has **no cargo dependent** — it is a
  workspace member nothing imports. A compiled program only links it when the
  program imports `perry/ui` on Linux, at which point
  `crates/perry/src/commands/compile/library_search.rs` looks for
  `libperry_ui_gtk4.a`. No gap fixture does that, so both arms of a gap A/B
  would link byte-identical archives and compare a compiler this change cannot
  reach. Running one would have produced a green table that means nothing —
  the shape of "the gate runs but its subject never did". The one way this
  change *can* reach other crates is the `tokio/tracing` feature removal above,
  and that is a feature nothing in the tree consumes. What *was* run instead is
  the shipping-path build with the new lockfile —
  `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`,
  exit 0 — which is what would catch a feature-unification break, and
  `cargo deny check licenses bans sources`, which is what would catch the new
  `task-local` crate.
* **No benchmark.** Nothing here is on a hot path, and the box was running two
  other lanes throughout.
* **No macOS or Windows arm.** The crate is `#![cfg(target_os = "linux")]` in
  its entirety and its dependencies are target-gated to Linux, so there is
  nothing to run there. `perry-ui-macos` / `perry-ui-windows` are untouched.

## What is not verified

Stated plainly, because a tray that compiles and never appears is the failure
this work had to avoid:

* **No StatusNotifierItem *host* was involved.** A build box has no desktop
  session. The test proves Perry publishes a correct SNI object and answers
  property reads and emits change signals; it does **not** prove an icon appears
  in a KDE/GNOME/XFCE panel. The code path that differs on a real desktop is
  ksni's watcher registration, which under `assume_sni_available(true)` (what
  Perry passes) routes a missing watcher to `Tray::watcher_offline` — the same
  branch this test exercises, since the box has no watcher either. What is
  untested is the *success* branch of `RegisterStatusNotifierItem` and
  everything the host does after it.
* **No media keys, no lock-screen widget.** Likewise: the MPRIS object is
  correct on the bus, but nothing was driven by a real
  `org.mpris.MediaPlayer2` consumer (GNOME's shell, `playerctl`, a Bluetooth
  headset). The D-Bus → `poll_tick` → GStreamer command path is unchanged by
  this work (it was already a `std::sync::mpsc`), but it was not exercised
  end-to-end.
* **No GTK main loop was running.** The test drives the FFI entry points
  directly; `app.run()` was never called, so the interaction between a live GTK
  main context and the new background thread is untested. It is also the
  interaction this change deliberately avoided creating: no work moved onto or
  off the main thread.
* **No long-running soak.** ksni's driver thread exits when its executor goes
  idle and restarts on the next spawn; the tray's service loop keeps it alive,
  so this should not happen while a tray exists, but nothing here ran for hours
  to confirm it.
* **The `webkit6` / `libshumate` / camera surfaces were not exercised at all.**
  They are in the same crate and were rebuilt, nothing more.

## turnloop gaps found

**None in turnloop itself — and that is the finding.** This lane removes a tokio
edge without adding a turnloop dependency: the two services are D-Bus clients
whose crates already carry executors, so the right answer was to stop asking for
the tokio backend rather than to route anything through Perry's loop. Worth
recording so the next reader of the inventory does not assume every remaining
edge needs a turnloop feature first.

One adjacent observation, if the coordinator wants it filed: Perry now runs
*two* async worlds on Linux UI builds — turnloop for JS I/O, and the smol
ecosystem (`async-io`, `async-executor`, `async-channel`) for the D-Bus
services, which brings its own reactor thread. Consolidating would need turnloop
to be able to host an arbitrary foreign `Future` from a non-JS thread, which it
cannot today. That is a real cost (three idle threads) but not a correctness
problem, and unifying it is much more work than this lane.

## Tooling note

`scripts/tokio_inventory.py`'s ungated `source_sites` regex matches `block_on`,
`spawn_blocking`, `Runtime::new` and `new_current_thread` as "tokio-shaped".
`block_on` is a generic async idiom — `futures_lite`, `async_io`, glib's
`MainContext` and `pollster` all have one — so a crate that has finished
migrating keeps a nonzero count and moves into the "tokio-shaped source but NO
manifest edge" list. The script already says this number is not load-bearing and
explains why; this is just the first crate to demonstrate it. No renaming was
done to make the number look better.

## For the integrator

- The acceptance command is `cargo build --release -p perry-ui-gtk4` on a box
  with the GTK 4 dev headers; PR CI will not build this crate and cannot catch a
  break here. `release-packages.yml` is the first thing that would.
- The D-Bus test is worth re-running on any machine that has a session bus:
  `dbus-run-session -- cargo test --release -p perry-ui-gtk4 --test dbus_services_without_tokio -- --nocapture`.
  It skips loudly without one — read the line, a skip is not a pass.
- **What would genuinely finish this**: one run on a real Linux desktop with a
  tray host (KDE Plasma, or GNOME with the AppIndicator extension) — create a
  tray, change its icon and tooltip, click it, and play something with
  `set_now_playing` then press a media key. That is fifteen minutes on a laptop
  and it is the only thing that closes the gap named above.
- The box tree is `/root/claude-turnloop-gtk4` (its own `target/`, `OWNER` file
  in place). `PERRY_RUNTIME_DIR` must be overridden per tree —
  `/etc/profile.d/perry.sh` points it at a different checkout. Delete the tree
  when done.
