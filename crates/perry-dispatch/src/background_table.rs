//! `PERRY_BACKGROUND_TABLE` — perry/background deferred / periodic work (issue #538).

use super::*;

/// perry/background — deferred / periodic background work (issue #538).
/// iOS BGTaskScheduler + Android WorkManager. Handler closures arrive via
/// `registerTask` and are persisted in a runtime-side identifier→closure
/// table so the platform's launchHandler / Worker can dispatch them.
/// `kind` is passed as a NaN-boxed string ("appRefresh" | "processing");
/// `earliestStartMs` is f64 (epoch ms or 0); `requiresNetwork` /
/// `requiresCharging` are NaN-boxed booleans.
pub static PERRY_BACKGROUND_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "registerTask",
        runtime: "perry_background_register_task",
        args: &[ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "schedule",
        runtime: "perry_background_schedule",
        args: &[
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cancel",
        runtime: "perry_background_cancel",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
];
