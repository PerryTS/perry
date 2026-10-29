//! Facts about the runtime archive's surface that the compiler reads too.
//!
//! The perry CLI must not link perry-runtime: the object cache keys every
//! object on a hash of the perry binary, so runtime code linked into it would
//! make every runtime edit miss every cached object. What the CLI needs to
//! know about the runtime lives here instead, and perry-runtime's build script
//! reads the same lists:
//!
//! - [`stubs`]: the no-op FFI stubs of the harmonyos build (#395, #399). The
//!   runtime build generates their bodies from it; `perry check` warns about
//!   the imports that resolve to one (#464).
//! - [`node_api_host_symbols`]: the Node-API host export inventory. The runtime
//!   build generates its retention anchors from it; the link exports it.

use perry_dispatch::{ArgKind, MethodRow, ReturnKind};

/// One no-op stub of the harmonyos runtime build.
#[derive(Copy, Clone, Debug)]
pub struct Stub {
    /// Runtime FFI symbol the stub body defines (`perry_ui_*`, …).
    pub symbol: &'static str,
    pub args: &'static [ArgKind],
    pub ret: ReturnKind,
    /// TypeScript-level name the user imports (`"keychainSave"`,
    /// `"notificationSchedule"`, …).
    pub ts_name: Option<&'static str>,
    /// Source TS module that exposes `ts_name` (`"perry/system"`, …).
    pub module: &'static str,
    /// Why this symbol is a stub (shown in the warning).
    pub reason: &'static str,
    /// Tracking issue for the stub family (`"#399"`, etc.).
    pub issue: Option<&'static str>,
}

/// Every stub, each symbol once: the rows of perry-dispatch's perry/ui,
/// perry/system and perry/updater tables, then the direct-call symbols codegen
/// emits outside those tables.
///
/// The stub list describes what the harmonyos build stubs whatever the host
/// is: `perry check` runs on the host but surfaces harmonyos stub warnings to a
/// developer cross-checking a harmonyos build.
pub fn stubs() -> Vec<Stub> {
    let tables: &[(&[MethodRow], &str)] = &[
        (perry_dispatch::PERRY_UI_TABLE, "perry/ui"),
        (perry_dispatch::PERRY_UI_INSTANCE_TABLE, "perry/ui"),
        (perry_dispatch::PERRY_SYSTEM_TABLE, "perry/system"),
        (perry_dispatch::PERRY_UPDATER_TABLE, "perry/updater"),
    ];
    let mut out: Vec<Stub> = Vec::new();
    let mut push = |symbol, args, ret, ts_name, module| {
        // Some dispatch rows reuse the same runtime symbol (e.g.
        // `scrollViewSetChild` and `scrollviewSetChild` both →
        // `perry_ui_scrollview_set_child`). Emit each symbol once.
        if out.iter().any(|stub| stub.symbol == symbol) {
            return;
        }
        let (reason, issue) = metadata_for(symbol);
        out.push(Stub {
            symbol,
            args,
            ret,
            ts_name,
            module,
            reason,
            issue,
        });
    };
    for (table, module) in tables {
        for row in table.iter() {
            if is_stubbable(row.runtime) {
                push(row.runtime, row.args, row.ret, Some(row.method), module);
            }
        }
    }
    for &(symbol, args, ret, ts_name, module) in DIRECT_CALL_STUBS {
        push(symbol, args, ret, ts_name, module);
    }
    out
}

/// Symbols owned by other modules in perry-runtime — stubbing them would be a
/// duplicate-symbol error at link time. Keep this in sync with
/// `crates/perry-runtime/src/i18n.rs` + `crates/perry-runtime/src/media_playback.rs`.
/// The `perry_i18n_` / `perry_media_` prefixes don't appear in the four tables
/// so this is belt-and-braces — the actual filter is the table list.
fn is_stubbable(runtime: &str) -> bool {
    runtime.starts_with("perry_ui_")
        || runtime.starts_with("perry_system_")
        || runtime.starts_with("perry_updater_")
        // `perry_get_device_idiom` is the one outlier in PERRY_SYSTEM_TABLE
        // that doesn't carry a `perry_system_` prefix. Allow it explicitly.
        || runtime == "perry_get_device_idiom"
}

/// Per-table reason text + tracking issue. Surfaced by `perry check` and
/// printed by the runtime's `perry_stub_warn` on first call.
fn metadata_for(symbol: &str) -> (&'static str, Option<&'static str>) {
    if symbol.starts_with("perry_ui_") || symbol == "perry_get_device_idiom" {
        (
            "harmonyos perry/ui FFI: lowered code outside the ArkUI harvest target's App body has no real backing",
            Some("#395"),
        )
    } else if symbol.starts_with("perry_system_") {
        (
            "harmonyos perry/system FFI: not implemented on this build",
            Some("#399"),
        )
    } else if symbol.starts_with("perry_updater_") {
        (
            "harmonyos perry/updater FFI: not implemented on this build",
            Some("#399"),
        )
    } else {
        ("no-op stub on this build", None)
    }
}

/// Direct-call FFI symbols that bypass the dispatch tables — codegen emits
/// these for the simple constructor shapes (`VStack(items)`, `HStack(items)`,
/// `Button(label, onPress)`) in `crates/perry-codegen/src/lower_call/native.rs`,
/// and for the trigger-shape-switched `notificationSchedule({trigger:{type,...}})`
/// in `crates/perry-codegen/src/lower_call.rs::lower_notification_schedule`.
/// They aren't in any PERRY_*_TABLE so the table walk misses them. Keep in
/// sync with the hardcoded callsites — `grep -h '"perry_(ui|system|updater)_' \
/// crates/perry-codegen/src/**/*.rs | sort -u`.
///
/// The `ts_name` column carries the user-facing TS function name when it
/// survives codegen as-is (`VStack`, `HStack`, `Button`). For the
/// notificationSchedule fan-out the TS-level name is the same
/// (`notificationSchedule`) for all three runtime variants — the trigger-shape
/// switch happens at lowering, not in source — so the static scan dedupes on
/// `ts_name` per module.
type DirectCallStub = (
    &'static str,
    &'static [ArgKind],
    ReturnKind,
    Option<&'static str>,
    &'static str,
);
const DIRECT_CALL_STUBS: &[DirectCallStub] = &[
    (
        "perry_ui_vstack_create",
        &[ArgKind::F64],
        ReturnKind::Widget,
        Some("VStack"),
        "perry/ui",
    ),
    (
        "perry_ui_hstack_create",
        &[ArgKind::F64],
        ReturnKind::Widget,
        Some("HStack"),
        "perry/ui",
    ),
    (
        "perry_ui_button_create",
        &[ArgKind::Str, ArgKind::Closure],
        ReturnKind::Widget,
        Some("Button"),
        "perry/ui",
    ),
    // notificationSchedule(...) — three trigger variants, dispatched at
    // compile time on `trigger.type`. Args are (id, title, body) string ptrs
    // as I64 then trigger-specific f64 fields. None of these appear in
    // PERRY_SYSTEM_TABLE (the table only carries the simpler
    // `notificationSend` shape).
    (
        "perry_system_notification_schedule_interval",
        &[
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ReturnKind::Void,
        Some("notificationSchedule"),
        "perry/system",
    ),
    (
        "perry_system_notification_schedule_calendar",
        &[ArgKind::Str, ArgKind::Str, ArgKind::Str, ArgKind::F64],
        ReturnKind::Void,
        Some("notificationSchedule"),
        "perry/system",
    ),
    (
        "perry_system_notification_schedule_location",
        &[
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ReturnKind::Void,
        Some("notificationSchedule"),
        "perry/system",
    ),
];

/// Checked-in Node v26.5.1 / N-API v8 export inventory of the runtime's
/// Node-API host (`crates/perry-runtime/src/node_api_host`).
pub const NODE_API_HOST_SYMBOLS: &str = include_str!("node_api_host_symbols.txt");

/// The symbol names of [`NODE_API_HOST_SYMBOLS`], without blank and `#` lines.
pub fn node_api_host_symbols() -> impl Iterator<Item = &'static str> {
    NODE_API_HOST_SYMBOLS
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_are_unique_and_complete() {
        let stubs = stubs();
        assert!(!stubs.is_empty());
        let mut symbols: Vec<&str> = stubs.iter().map(|stub| stub.symbol).collect();
        symbols.sort_unstable();
        symbols.dedup();
        assert_eq!(symbols.len(), stubs.len(), "a stub symbol is listed twice");
        for stub in &stubs {
            assert!(is_stubbable(stub.symbol), "{}", stub.symbol);
            assert!(!stub.module.is_empty() && !stub.reason.is_empty());
        }
    }

    #[test]
    fn node_api_inventory_is_symbol_names() {
        assert!(node_api_host_symbols().count() > 100);
        for name in node_api_host_symbols() {
            assert!(
                name.starts_with("napi_") || name.starts_with("node_api_"),
                "{name}"
            );
        }
    }
}

mod realm_exports;
pub mod realm_json;
/// Shared ABI for typed manifest consumers, including build scripts.
pub use perry_abi as abi;
pub mod realm_layouts;
pub mod realm_source;
mod realm_storage;
