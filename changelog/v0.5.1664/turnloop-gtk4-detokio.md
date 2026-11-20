Take tokio out of `perry-ui-gtk4`, the Linux GTK4 UI backend, without dropping
the Linux system tray or MPRIS media-key support. The tokio inventory
(`scripts/tokio_inventory.py`) goes from 39 manifest edges to 38, and its
removal-plan group **M** is now empty.

Group M's entry read "`ksni` and `mpris-server` REQUIRE tokio (they are zbus
clients with a `tokio` feature). Removing it means replacing both crates or
dropping tray/MPRIS support on Linux." That was wrong in both halves, and the
manifests say so:

* `ksni` has an `async-io` feature that is a peer of its `tokio` default — the
  two are mutually exclusive (`ksni::compat` refuses to compile with both). In
  `async-io` mode ksni owns an `async_executor` driver thread of its own and
  builds its zbus connection with `internal_executor(false)`, so it needs no
  ambient runtime at all.
* `mpris-server`'s `tokio` feature is opt-in, is not in its defaults, and only
  forwards to `zbus/tokio`. With it off, zbus runs on `async-io` and starts its
  own `zbus::Connection executor` thread.

Perry had asked for `features = ["tokio"]` on both and then carried a direct
`tokio` dependency to feed them — three uses in two files. Those become a
28-line `background` module: `futures-lite`'s `block_on` for the two one-shot
handshakes (`ksni`'s `spawn()`, `mpris_server::Server::new()`), and one
`async-executor` thread, started lazily, for the tray's fire-and-forget
property refreshes. The MPRIS update queue moves from
`tokio::sync::mpsc::unbounded_channel` to `async-channel`. Every thread that
work ran on before still runs it, and nothing moved onto the GTK main thread.

`zbus` therefore compiles without its `tokio` feature for the whole workspace,
which also drops `tokio/tracing` (zbus was its only enabler; nothing consumes
the instrumentation). One new transitive crate, `task-local` 0.1.1
(MIT OR Apache-2.0), arrives with ksni's `async-io` feature.

No CI tier builds this crate — the runners have no `libgtk-4-dev` — so the
change ships with `crates/perry-ui-gtk4/tests/dbus_services_without_tokio.rs`,
a headless `dbus-run-session` test that claims the tray's
`org.kde.StatusNotifierItem-<pid>-1` name and reads its `Id` back, proves a
`set_tooltip` reaches the bus as a `NewToolTip` signal (the property is
computed on demand, so only the signal shows the fire-and-forget update ran),
brings up `org.mpris.MediaPlayer2.perry-<pid>` and checks the pushed title
arrives in a `PropertiesChanged`, and asserts the process has no tokio worker
thread. It skips loudly without a session bus.
