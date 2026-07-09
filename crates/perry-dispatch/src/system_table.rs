//! `PERRY_SYSTEM_TABLE` — perry/system calls.

use super::*;

pub static PERRY_SYSTEM_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "isDarkMode",
        runtime: "perry_system_is_dark_mode",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getDeviceIdiom",
        runtime: "perry_get_device_idiom",
        args: &[],
        ret: ReturnKind::F64,
    },
    // #1475 — safe-area insets. Returns `{ top, right, bottom, left }` (points)
    // read from `UIWindow.safeAreaInsets` (iOS) / `WindowInsets.systemBars()`
    // (Android), zero on macOS/host. The platform FFI returns the object
    // already NaN-boxed, so the row uses `ReturnKind::F64` (pass-through).
    MethodRow {
        method: "getSafeAreaInsets",
        runtime: "perry_system_get_safe_area_insets",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "openURL",
        runtime: "perry_system_open_url",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // #917 — system share sheet. Both entry points take a body
    // string + an optional title (empty = no title); the platform
    // implementation maps to the native share API
    // (UIActivityViewController / NSSharingServicePicker /
    // Intent.ACTION_SEND).
    MethodRow {
        method: "shareText",
        runtime: "perry_system_share_text",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "shareUrl",
        runtime: "perry_system_share_url",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // #675 — App Group / cross-process shared storage. set/get/delete
    // map to the platform's native shared-storage suite on Apple
    // platforms (`UserDefaults(suiteName:)`); other platforms get an
    // in-process HashMap fallback for API parity.
    MethodRow {
        method: "appGroupSet",
        runtime: "perry_system_app_group_set",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "appGroupGet",
        runtime: "perry_system_app_group_get",
        args: &[ArgKind::Str],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "appGroupDelete",
        runtime: "perry_system_app_group_delete",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "keychainSave",
        runtime: "perry_system_keychain_save",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "keychainGet",
        runtime: "perry_system_keychain_get",
        args: &[ArgKind::Str],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "keychainDelete",
        runtime: "perry_system_keychain_delete",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "preferencesGet",
        runtime: "perry_system_preferences_get",
        args: &[ArgKind::Str],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "preferencesSet",
        runtime: "perry_system_preferences_set",
        args: &[ArgKind::Str, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationSend",
        runtime: "perry_system_notification_send",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationRegisterRemote",
        runtime: "perry_system_notification_register_remote",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationOnReceive",
        runtime: "perry_system_notification_on_receive",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationOnBackgroundReceive",
        runtime: "perry_system_notification_on_background_receive",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationCancel",
        runtime: "perry_system_notification_cancel",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationOnTap",
        runtime: "perry_system_notification_on_tap",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioStart",
        runtime: "perry_system_audio_start",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "audioStop",
        runtime: "perry_system_audio_stop",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioGetLevel",
        runtime: "perry_system_audio_get_level",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "audioGetPeak",
        runtime: "perry_system_audio_get_peak",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "audioGetWaveform",
        runtime: "perry_system_audio_get_waveform",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    // Returns the device model identifier (e.g. "iPhone15,2") as a JS
    // string. The runtime fn returns a raw `*mut StringHeader` (i64) via
    // `js_string_from_bytes`, so the return kind MUST be `Str` (NaN-box
    // with STRING_TAG) — same as `getLocale` below. `F64` would pass the
    // raw pointer bits through as a double → `NaN`, and any downstream use
    // (e.g. `table[getDeviceModel()]`) then dereferences NaN as a string
    // pointer and segfaults (#5972).
    MethodRow {
        method: "getDeviceModel",
        runtime: "perry_system_get_device_model",
        args: &[],
        ret: ReturnKind::Str,
    },
    // Bug-report-flow utility: stable OS-version string per platform,
    // for splicing into crash reports / telemetry. Same raw
    // `*mut StringHeader` return shape as `getDeviceModel` — must be
    // `Str`, not `F64` (#5972).
    MethodRow {
        method: "getOSVersion",
        runtime: "perry_system_get_os_version",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "audioSetOutputFilename",
        runtime: "perry_system_audio_set_output_filename",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioRegisterCallback",
        runtime: "perry_system_audio_register_callback",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioUnregisterCallback",
        runtime: "perry_system_audio_unregister_callback",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioStartRecording",
        runtime: "perry_system_audio_start_recording",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioStopRecording",
        runtime: "perry_system_audio_stop_recording",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "getLocale",
        runtime: "perry_system_get_locale",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getAppVersion",
        runtime: "perry_system_get_app_version",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getAppBuildNumber",
        runtime: "perry_system_get_app_build_number",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getBundleId",
        runtime: "perry_system_get_bundle_id",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getAppIcon",
        runtime: "perry_system_get_app_icon",
        args: &[ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    // ---- Geolocation (issue #552) ----
    MethodRow {
        method: "geolocationGetCurrent",
        runtime: "perry_system_geolocation_get_current",
        args: &[ArgKind::Closure, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "geolocationWatch",
        runtime: "perry_system_geolocation_watch",
        args: &[ArgKind::Closure],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "geolocationStopWatch",
        runtime: "perry_system_geolocation_stop_watch",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "geolocationRequestPermission",
        runtime: "perry_system_geolocation_request_permission",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- Photo-library image picker (issue #552) ----
    MethodRow {
        method: "imagePickerPick",
        runtime: "perry_system_image_picker_pick",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- In-app screen capture (issue #918) ----
    MethodRow {
        method: "takeScreenshot",
        runtime: "perry_system_take_screenshot",
        args: &[],
        ret: ReturnKind::Str,
    },
    // ---- Network reachability (issue #582) ----
    MethodRow {
        method: "networkGetStatus",
        runtime: "perry_system_network_get_status",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "networkOnChange",
        runtime: "perry_system_network_on_change",
        args: &[ArgKind::Closure],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "networkStopOnChange",
        runtime: "perry_system_network_stop_on_change",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Deep links: Universal Links / App Links / URL schemes (issue #583) ----
    MethodRow {
        method: "appOnOpenUrl",
        runtime: "perry_system_app_on_open_url",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "appGetLaunchUrl",
        runtime: "perry_system_app_get_launch_url",
        args: &[],
        ret: ReturnKind::Str,
    },
];
