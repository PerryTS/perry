//! `PERRY_UPDATER_TABLE` — perry/updater desktop self-update wrappers.

use super::*;

/// Maps the TS exports from `types/perry/updater/index.d.ts` to their
/// `perry_updater_*` runtime symbols. Desktop-only by design — mobile
/// updates go through the OS store, not self-update. The runtime
/// symbols live in `perry-updater` (split internally into `core` for
/// cross-platform helpers and `desktop` for per-OS install/relaunch).
///
/// i64 returns use `I64AsF64` because the Rust impls return `i64` and the
/// codegen converts via `sitofp` to a NaN-boxable JS number. Strings flow
/// through `Str` (raw `*StringHeader` ptr extracted via
/// `js_get_string_pointer_unified` on the codegen side).
pub static PERRY_UPDATER_TABLE: &[MethodRow] = &[
    // perry-updater::core — pure cross-platform helpers.
    MethodRow {
        method: "compareVersions",
        runtime: "perry_updater_compare_versions",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "verifyHash",
        runtime: "perry_updater_verify_hash",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "verifySignature",
        runtime: "perry_updater_verify_signature",
        args: &[ArgKind::Str, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "verifySignatureV2",
        runtime: "perry_updater_verify_signature_v2",
        args: &[ArgKind::Str, ArgKind::Str, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "computeFileSha256",
        runtime: "perry_updater_compute_file_sha256",
        args: &[ArgKind::Str],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "writeSentinel",
        runtime: "perry_updater_write_sentinel",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "readSentinel",
        runtime: "perry_updater_read_sentinel",
        args: &[ArgKind::Str],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "clearSentinel",
        runtime: "perry_updater_clear_sentinel",
        args: &[ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    // perry-updater::desktop — platform-touching helpers.
    MethodRow {
        method: "getExePath",
        runtime: "perry_updater_get_exe_path",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getBackupPath",
        runtime: "perry_updater_get_backup_path",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getSentinelPath",
        runtime: "perry_updater_get_sentinel_path",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "installUpdate",
        runtime: "perry_updater_install",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "performRollback",
        runtime: "perry_updater_perform_rollback",
        args: &[ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    // relaunch returns the spawned PID as f64 (or -1.0 on error).
    MethodRow {
        method: "relaunch",
        runtime: "perry_updater_relaunch",
        args: &[ArgKind::Str],
        ret: ReturnKind::F64,
    },
];
