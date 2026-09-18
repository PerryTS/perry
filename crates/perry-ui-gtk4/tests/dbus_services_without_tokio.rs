//! Headless proof that this crate's two D-Bus services are *live* without tokio.
//!
//! `perry-ui-gtk4` used to carry a direct `tokio` dependency solely to own a
//! runtime for `ksni` (the StatusNotifierItem tray) and `mpris-server` (media
//! keys / lock-screen). Both now run on the `smol` side: `ksni` on its
//! `async-io` feature with its own executor thread, `mpris-server` on a zbus
//! built without `zbus/tokio`.
//!
//! "It compiles" is not evidence for that change. A tray that compiles and
//! never appears is precisely the failure this has to avoid, and every failure
//! mode of getting the executor wrong — a future nobody polls, a zbus that
//! panics looking for a reactor, a fire-and-forget update that is dropped on
//! the floor — is invisible to the compiler and to a unit test. So this test
//! talks to a real D-Bus session bus and checks the three things only a live
//! bus can show:
//!
//! 1. the tray's `org.kde.StatusNotifierItem-<pid>-1` name is **claimed** and
//!    answers a `GetAll` with the `Id` Perry gave it (the ksni service loop and
//!    its zbus connection are running);
//! 2. a `set_tooltip` produces a `NewToolTip` **signal** (the fire-and-forget
//!    `background::spawn_detached` path ran: the property itself is computed on
//!    demand, so only the signal proves the update round trip happened, not a
//!    re-read);
//! 3. `set_now_playing` brings up `org.mpris.MediaPlayer2.perry-<pid>` and emits
//!    a `PropertiesChanged` carrying the metadata (the `async-channel` hop to
//!    the `perry-mpris` thread and zbus's own connection thread both work).
//!
//! It also asserts on the process's live thread names: the services must be
//! served by `async-io`/`zbus`/`perry-*` threads and by **no** tokio worker.
//!
//! # Running it
//!
//! Needs a session bus, and no CI tier builds this crate at all (`test.yml`
//! excludes it — the runners have no `libgtk-4-dev`), so this is run by hand on
//! a machine with the GTK 4 dev headers:
//!
//! ```sh
//! dbus-run-session -- cargo test --release -p perry-ui-gtk4 --test dbus_services_without_tokio -- --nocapture
//! ```
//!
//! With no `DBUS_SESSION_BUS_ADDRESS` it skips loudly rather than failing, so
//! `cargo test -p perry-ui-gtk4` stays green on a bus-less box. A skip is not a
//! pass: read the printed line.

#![cfg(target_os = "linux")]

use futures_lite::future::block_on;
use futures_lite::StreamExt;
use mpris_server::zbus;
use perry_ffi::StringHeader;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use zbus::zvariant::OwnedValue;

/// Build the `StringHeader`-prefixed buffer the FFI entry points take.
///
/// Same shape as `perry-ffi`'s own `copy_string_from_raw` test fixture: the
/// header is followed immediately by the UTF-8 payload. Built by hand rather
/// than through `js_string_from_bytes` so the test needs no initialised Perry
/// arena — these strings are copied out on entry and never retained.
fn js_str(s: &str) -> Vec<u32> {
    let bytes = s.as_bytes();
    let header_len = std::mem::size_of::<StringHeader>();
    let words = (header_len + bytes.len())
        .div_ceil(std::mem::size_of::<u32>())
        .max(1);
    let mut storage = vec![0_u32; words];
    let header = StringHeader {
        utf16_len: s.encode_utf16().count() as u32,
        byte_len: bytes.len() as u32,
        capacity: bytes.len() as u32,
        refcount: 1,
        flags: 0,
    };
    // SAFETY: `Vec<u32>` is 4-byte aligned and `words` covers header + payload.
    unsafe {
        storage.as_mut_ptr().cast::<StringHeader>().write(header);
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            storage.as_mut_ptr().cast::<u8>().add(header_len),
            bytes.len(),
        );
    }
    storage
}

fn list_names(conn: &zbus::Connection) -> Vec<String> {
    let msg = block_on(conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "ListNames",
        &(),
    ))
    .expect("ListNames call");
    msg.body()
        .deserialize::<Vec<String>>()
        .expect("ListNames body")
}

fn wait_for_name(conn: &zbus::Connection, name: &str, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if list_names(conn).iter().any(|n| n == name) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Subscribe NOW, return a handle that waits for the first matching signal.
///
/// Subscribing has to happen before the call that triggers the signal, or the
/// test races the very thing it is measuring.
struct SignalWatch {
    rx: mpsc::Receiver<Option<zbus::Message>>,
}

impl SignalWatch {
    fn subscribe(interface: &str, member: &str, path: &str) -> Self {
        let conn = block_on(zbus::Connection::session()).expect("session bus");
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(interface.to_string())
            .expect("interface")
            .member(member.to_string())
            .expect("member")
            .path(path.to_string())
            .expect("path")
            .build();
        let mut stream =
            block_on(zbus::MessageStream::for_match_rule(rule, &conn, None)).expect("match rule");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // `conn` is moved in so the subscription outlives this scope.
            let _conn = conn;
            let msg = block_on(stream.next()).and_then(|m| m.ok());
            let _ = tx.send(msg);
        });
        Self { rx }
    }

    fn wait(self, within: Duration) -> Option<zbus::Message> {
        self.rx.recv_timeout(within).ok().flatten()
    }
}

fn thread_names() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc/self/task") {
        for entry in entries.flatten() {
            if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
                out.push(comm.trim().to_string());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn tray_and_mpris_serve_a_real_bus_without_tokio() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!(
            "SKIPPED (not a pass): no DBUS_SESSION_BUS_ADDRESS. Re-run as\n  \
             dbus-run-session -- cargo test --release -p perry-ui-gtk4 \
             --test dbus_services_without_tokio -- --nocapture"
        );
        return;
    }

    let pid = std::process::id();
    let conn = block_on(zbus::Connection::session()).expect("session bus");

    // ---- 1. the tray registers and answers ------------------------------
    let icon = js_str("");
    let tray = perry_ui_gtk4::tray::create(icon.as_ptr().cast());
    assert!(
        tray > 0,
        "tray::create returned 0 — the KSNI service did not start"
    );

    let sni_name = format!("org.kde.StatusNotifierItem-{pid}-1");
    assert!(
        wait_for_name(&conn, &sni_name, Duration::from_secs(10)),
        "{sni_name} never appeared on the bus; names were {:?}",
        list_names(&conn)
    );

    let msg = block_on(conn.call_method(
        Some(sni_name.as_str()),
        "/StatusNotifierItem",
        Some("org.freedesktop.DBus.Properties"),
        "GetAll",
        &("org.kde.StatusNotifierItem",),
    ))
    .expect("GetAll on the tray item");
    let props: HashMap<String, OwnedValue> = msg.body().deserialize().expect("GetAll body");
    let id = props
        .get("Id")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    assert_eq!(
        id,
        format!("perry-tray-{pid}-1"),
        "the tray answered GetAll, but with the wrong Id: {props:?}"
    );
    println!("tray: {sni_name} is live, Id = {id}");

    // ---- 2. an update actually round-trips -------------------------------
    // The properties themselves are computed on demand, so a re-read proves
    // nothing. Only the signal shows that `refresh()` -> spawn_detached ->
    // Handle::update -> the ksni service loop ran.
    let watch = SignalWatch::subscribe(
        "org.kde.StatusNotifierItem",
        "NewToolTip",
        "/StatusNotifierItem",
    );
    let tip = js_str("perry turnloop smoke");
    perry_ui_gtk4::tray::set_tooltip(tray, tip.as_ptr().cast());
    assert!(
        watch.wait(Duration::from_secs(15)).is_some(),
        "no NewToolTip signal: the fire-and-forget tray update never ran"
    );
    println!("tray: NewToolTip observed — background::spawn_detached is driving futures");

    // ---- 3. MPRIS comes up and pushes metadata ---------------------------
    let mpris_watch = SignalWatch::subscribe(
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        "/org/mpris/MediaPlayer2",
    );
    let (title, artist, album, art) = (
        js_str("Perry Smoke Track"),
        js_str("turnloop"),
        js_str("gtk4"),
        js_str(""),
    );
    perry_ui_gtk4::media_playback::set_now_playing(
        0.0,
        title.as_ptr().cast(),
        artist.as_ptr().cast(),
        album.as_ptr().cast(),
        art.as_ptr().cast(),
    );

    let mpris_name = format!("org.mpris.MediaPlayer2.perry-{pid}");
    assert!(
        wait_for_name(&conn, &mpris_name, Duration::from_secs(10)),
        "{mpris_name} never appeared; names were {:?}",
        list_names(&conn)
    );
    println!("mpris: {mpris_name} is live");

    let changed = mpris_watch
        .wait(Duration::from_secs(15))
        .expect("no PropertiesChanged from the MPRIS server — the update queue never drained");
    let (iface, props, _invalidated): (String, HashMap<String, OwnedValue>, Vec<String>) = changed
        .body()
        .deserialize()
        .expect("PropertiesChanged body");
    assert_eq!(iface, "org.mpris.MediaPlayer2.Player");
    let metadata = props
        .get("Metadata")
        .unwrap_or_else(|| panic!("PropertiesChanged carried no Metadata: {props:?}"));
    let rendered = format!("{metadata:?}");
    assert!(
        rendered.contains("Perry Smoke Track"),
        "the metadata push did not carry the title: {rendered}"
    );
    println!("mpris: PropertiesChanged carried the pushed title");

    // ---- 4. and none of it is running on tokio ---------------------------
    let threads = thread_names();
    println!("threads: {threads:?}");
    assert!(
        !threads.iter().any(|t| t.starts_with("tokio-")),
        "a tokio runtime thread is live in a build that has no tokio dependency: {threads:?}"
    );
    for expected in ["perry-ui-async", "perry-mpris"] {
        assert!(
            threads.iter().any(|t| t == expected),
            "expected a {expected} thread; saw {threads:?}"
        );
    }

    perry_ui_gtk4::tray::destroy(tray);
}
