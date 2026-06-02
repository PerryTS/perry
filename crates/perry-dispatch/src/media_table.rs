//! `PERRY_MEDIA_TABLE` — perry/media streaming playback wrappers.

use super::*;

/// perry/media — streaming media playback (`createPlayer`, `play`, `pause`,
/// `seek`, `setVolume`, `onStateChange`, `onTimeUpdate`, `setNowPlaying`,
/// `destroy`). Backed by AVPlayer on Apple, MediaPlayer/JNI on Android,
/// GStreamer on GTK4/Linux, Media Foundation on Windows, and `<audio>` on
/// the web target.
pub static PERRY_MEDIA_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "createPlayer",
        runtime: "perry_media_create_player",
        args: &[ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "play",
        runtime: "perry_media_play",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "pause",
        runtime: "perry_media_pause",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stop",
        runtime: "perry_media_stop",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "seek",
        runtime: "perry_media_seek",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setVolume",
        runtime: "perry_media_set_volume",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setRate",
        runtime: "perry_media_set_rate",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "getCurrentTime",
        runtime: "perry_media_get_current_time",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getDuration",
        runtime: "perry_media_get_duration",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getState",
        runtime: "perry_media_get_state",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "isPlaying",
        runtime: "perry_media_is_playing",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "onStateChange",
        runtime: "perry_media_on_state_change",
        args: &[ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "onTimeUpdate",
        runtime: "perry_media_on_time_update",
        args: &[ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setNowPlaying",
        runtime: "perry_media_set_now_playing",
        args: &[
            ArgKind::F64,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Str,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "destroy",
        runtime: "perry_media_destroy",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
];
