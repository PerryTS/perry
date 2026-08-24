//! `PERRY_IOS_TABLE` — iOS-specific adaptive layout and Foundation Models.

use super::*;

/// APIs that intentionally expose iOS-only platform capabilities. Keeping
/// these out of `PERRY_UI_TABLE` prevents other UI backends from having to
/// pretend that UIKit scene geometry or Foundation Models exist.
pub static PERRY_IOS_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "getLayoutEnvironment",
        runtime: "perry_ios_get_layout_environment",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "onLayoutChange",
        runtime: "perry_ios_on_layout_change",
        args: &[ArgKind::Closure],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "offLayoutChange",
        runtime: "perry_ios_off_layout_change",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "foundationModelAvailability",
        runtime: "perry_ios_foundation_model_availability",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "createLanguageModelSession",
        runtime: "perry_ios_foundation_model_session_create",
        args: &[ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "respond",
        runtime: "perry_ios_foundation_model_respond",
        args: &[ArgKind::F64, ArgKind::Str],
        ret: ReturnKind::Promise,
    },
    MethodRow {
        method: "destroyLanguageModelSession",
        runtime: "perry_ios_foundation_model_session_destroy",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
];
