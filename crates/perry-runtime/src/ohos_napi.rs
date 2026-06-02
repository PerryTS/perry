//! HarmonyOS NAPI entry wrapper.
//!
//! HarmonyOS NEXT apps ship as `.so` libraries loaded by the ArkTS runtime,
//! not as standalone executables. When ArkTS executes
//! `import entry from 'libperry_app.so'`, the loader invokes each module's
//! `nm_register_func` (set up via `napi_module_register`) to populate the
//! `exports` object. We expose one function, `run`, which calls Perry's
//! compiled `main()` and returns its exit code to ArkTS as an `int32`.
//!
//! Registration happens via `.init_array` — Rust's equivalent of
//! `__attribute__((constructor))` — so it runs automatically on `dlopen`.
//! The TS entry is *not* invoked at load; ArkTS must explicitly call
//! `entry.run()` from its `EntryAbility.onCreate` (see the ArkTS shim
//! emitted by the compiler alongside the `.so`).
//!
//! Multi-call semantics: `entry.run()` calls `main()` every time, which
//! re-runs module init + user code. For the logic-only v1, that's the
//! correct shape — ArkTS calls it once from `onCreate`. A future
//! lifecycle-aware mode would need a guard to make re-entry a no-op
//! (or restart-friendly), but that's out of scope here.

use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::ptr;

#[repr(C)]
pub struct NapiEnv(());
#[repr(C)]
pub struct NapiValue(());
#[repr(C)]
pub struct NapiCallbackInfo(());

pub type NapiStatus = c_int;
pub type NapiCallback =
    unsafe extern "C" fn(env: *mut NapiEnv, info: *mut NapiCallbackInfo) -> *mut NapiValue;

#[repr(C)]
pub struct NapiModule {
    pub nm_version: c_int,
    pub nm_flags: c_uint,
    pub nm_filename: *const c_char,
    pub nm_register_func:
        Option<unsafe extern "C" fn(env: *mut NapiEnv, exports: *mut NapiValue) -> *mut NapiValue>,
    pub nm_modname: *const c_char,
    pub nm_priv: *mut c_void,
    pub reserved: [*mut c_void; 4],
}

// SAFETY: NapiModule is only mutated once during .init_array execution,
// before any ArkTS thread can observe it. After that it's read-only.
unsafe impl Sync for NapiModule {}

extern "C" {
    pub fn napi_module_register(m: *mut NapiModule);
    pub fn napi_create_int32(
        env: *mut NapiEnv,
        value: i32,
        result: *mut *mut NapiValue,
    ) -> NapiStatus;
    pub fn napi_create_function(
        env: *mut NapiEnv,
        utf8name: *const c_char,
        length: usize,
        cb: NapiCallback,
        data: *mut c_void,
        result: *mut *mut NapiValue,
    ) -> NapiStatus;
    pub fn napi_set_named_property(
        env: *mut NapiEnv,
        object: *mut NapiValue,
        utf8name: *const c_char,
        value: *mut NapiValue,
    ) -> NapiStatus;
    pub fn napi_get_cb_info(
        env: *mut NapiEnv,
        info: *mut NapiCallbackInfo,
        argc: *mut usize,
        argv: *mut *mut NapiValue,
        this_arg: *mut *mut NapiValue,
        data: *mut *mut c_void,
    ) -> NapiStatus;
    pub fn napi_get_value_int32(
        env: *mut NapiEnv,
        value: *mut NapiValue,
        result: *mut i32,
    ) -> NapiStatus;
    pub fn napi_get_undefined(env: *mut NapiEnv, result: *mut *mut NapiValue) -> NapiStatus;
    pub fn napi_create_string_utf8(
        env: *mut NapiEnv,
        str_: *const c_char,
        length: usize,
        result: *mut *mut NapiValue,
    ) -> NapiStatus;
    pub fn napi_create_object(env: *mut NapiEnv, result: *mut *mut NapiValue) -> NapiStatus;
    pub fn napi_typeof(
        env: *mut NapiEnv,
        value: *mut NapiValue,
        result: *mut c_int, // NapiValueType: 0=undefined,1=null,2=bool,3=number,4=string,5=symbol,6=object,7=function,8=external
    ) -> NapiStatus;
    pub fn napi_get_value_bool(
        env: *mut NapiEnv,
        value: *mut NapiValue,
        result: *mut bool,
    ) -> NapiStatus;
    pub fn napi_get_value_double(
        env: *mut NapiEnv,
        value: *mut NapiValue,
        result: *mut f64,
    ) -> NapiStatus;
    pub fn napi_get_value_string_utf8(
        env: *mut NapiEnv,
        value: *mut NapiValue,
        buf: *mut c_char,
        bufsize: usize,
        result: *mut usize,
    ) -> NapiStatus;
    pub fn napi_get_boolean(
        env: *mut NapiEnv,
        value: bool,
        result: *mut *mut NapiValue,
    ) -> NapiStatus;
}

// Perry's compiled entry. The TypeScript compiler always emits `main`
// (module init + user top-level code). On HarmonyOS we don't use it as
// the process entry — it's just a regular exported function that the
// NAPI `run` callback invokes.
//
// `-Wl,-Bsymbolic` on the link line ensures this resolves to our own
// `main`, not the ArkTS host process's `main`.
extern "C" {
    fn main() -> c_int;
}

unsafe extern "C" fn run(env: *mut NapiEnv, _info: *mut NapiCallbackInfo) -> *mut NapiValue {
    let exit_code = main();
    let mut out: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_int32(env, exit_code, &mut out);
    out
}

// Phase 2 v2 callback bridge. ArkUI's auto-emitted `.onClick(() =>
// perryEntry.invokeCallback(idx))` lands here. We read the int32 idx,
// dispatch to `perry_arkts_invoke_callback` (which unboxes the registered
// closure pointer and calls js_closure_call0), and return undefined.
//
// Multi-arg variants (Toggle/TextField/Slider value forwarding) are v2.5
// follow-ups — they need NaN-box marshaling on the way in.
unsafe extern "C" fn invoke_callback(
    env: *mut NapiEnv,
    info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    crate::arkts_callbacks::arkts_log_napi("invokeCallback NAPI ENTER");
    let mut argc: usize = 1;
    let mut argv: [*mut NapiValue; 1] = [ptr::null_mut(); 1];
    let _ = napi_get_cb_info(
        env,
        info,
        &mut argc,
        argv.as_mut_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
    );
    let mut idx_i32: i32 = -1;
    if argc >= 1 && !argv[0].is_null() {
        let _ = napi_get_value_int32(env, argv[0], &mut idx_i32);
    }
    crate::arkts_callbacks::arkts_log_napi(&format!(
        "invokeCallback NAPI argc={} idx={}",
        argc, idx_i32
    ));
    if idx_i32 >= 0 {
        let _ = crate::arkts_callbacks::perry_arkts_invoke_callback(idx_i32 as i64);
    }
    let mut undef: *mut NapiValue = ptr::null_mut();
    let _ = napi_get_undefined(env, &mut undef);
    undef
}

// Phase 2 v3 Option 1: drain one queued toast message and return it as
// a JS string (or undefined when empty). The auto-emitted .ets onClick
// loops calling this until it sees undefined, dispatching each entry to
// `promptAction.showToast({ message })` so the user sees the popup.
unsafe extern "C" fn drain_toast(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let bits = crate::arkts_callbacks::perry_arkts_drain_toast();
    // TAG_UNDEFINED → return JS undefined to the caller so its loop ends.
    if bits.to_bits() == 0x7FFC_0000_0000_0001 {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    }
    // STRING_TAG-boxed *StringHeader. Decode via the same payload-after-
    // header layout used elsewhere in the runtime, then create an N-API
    // utf8 string. byte_len fits in u32 by spec.
    let header = (bits.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::string::StringHeader;
    if header.is_null() {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    }
    let blen = (*header).byte_len as usize;
    let data_ptr = (header as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    let mut s: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_string_utf8(env, data_ptr as *const c_char, blen, &mut s);
    s
}

// Phase 2 v3 Option 2: drain one queued (id, value) text update and
// return it as a JS object `{ id, value }` (or undefined when empty).
// The auto-emitted .ets onClick loops calling this and applies each
// entry to the matching `@State text_<id>` for reactive Text rerendering.
//
// We pop directly from the Rust-side queue (no Perry-object roundtrip)
// and build the JS object inline via napi_create_object +
// napi_set_named_property. Avoids the interned-string-key trap from
// the previous shape — `js_object_set_field_by_name` keys by
// pointer-equality on interned StringHeaders, so reading back through
// `js_object_get_field_by_name_f64` with freshly-allocated keys (not
// pointer-equal to the originals) silently returned undefined fields.
unsafe extern "C" fn drain_text_update(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let Some((id, value)) = crate::arkts_callbacks::pop_text_update() else {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    };
    let mut id_napi: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_string_utf8(env, id.as_ptr() as *const c_char, id.len(), &mut id_napi);
    let mut val_napi: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_string_utf8(
        env,
        value.as_ptr() as *const c_char,
        value.len(),
        &mut val_napi,
    );
    let mut obj: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_object(env, &mut obj);
    let _ = napi_set_named_property(env, obj, b"id\0".as_ptr() as *const c_char, id_napi);
    let _ = napi_set_named_property(env, obj, b"value\0".as_ptr() as *const c_char, val_napi);
    obj
}

// Phase 2 v3.5: drain handler for the visibility-update queue.
// `crates/perry-codegen-arkts/src/lib.rs::wrap_index_page` emits a drain
// loop in every onClick body; this handler is what `drainVisibilityUpdate`
// in the .ets file resolves to. Returns `{id: string, hidden: boolean}`
// or `undefined` when the queue is empty.
unsafe extern "C" fn drain_visibility_update(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let Some((id, hidden)) = crate::arkts_callbacks::pop_visibility_update() else {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    };
    let mut id_napi: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_string_utf8(env, id.as_ptr() as *const c_char, id.len(), &mut id_napi);
    let mut hidden_napi: *mut NapiValue = ptr::null_mut();
    let _ = napi_get_boolean(env, hidden, &mut hidden_napi);
    let mut obj: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_object(env, &mut obj);
    let _ = napi_set_named_property(env, obj, b"id\0".as_ptr() as *const c_char, id_napi);
    let _ = napi_set_named_property(env, obj, b"hidden\0".as_ptr() as *const c_char, hidden_napi);
    obj
}

// Phase 2 v3.6: drain handler for the content-view-update queue.
// `crates/perry-codegen-arkts/src/lib.rs::wrap_index_page` emits a drain
// loop in every onClick body; this handler is what `drainContentViewUpdate`
// in the .ets file resolves to. Returns `{id: string, view: string}`
// (target_synth + view_id) or `undefined` when the queue is empty.
unsafe extern "C" fn drain_content_view_update(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let Some((id, view)) = crate::arkts_callbacks::pop_content_view_update() else {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    };
    let mut id_napi: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_string_utf8(env, id.as_ptr() as *const c_char, id.len(), &mut id_napi);
    let mut view_napi: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_string_utf8(
        env,
        view.as_ptr() as *const c_char,
        view.len(),
        &mut view_napi,
    );
    let mut obj: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_object(env, &mut obj);
    let _ = napi_set_named_property(env, obj, b"id\0".as_ptr() as *const c_char, id_napi);
    let _ = napi_set_named_property(env, obj, b"view\0".as_ptr() as *const c_char, view_napi);
    obj
}

// Phase 2 v2.5: invoke a registered closure with one value arg. ArkUI's
// Toggle / TextField / Slider onChange handlers call this with the
// event payload (boolean / string / number); we marshal it to a
// NaN-boxed f64 and dispatch via `perry_arkts_invoke_callback1`, which
// calls js_closure_call1 with the original Perry closure body.
//
// Marshaling rules:
//   - boolean → NaN-boxed TAG_TRUE (0x7FFC_0000_0000_0004) /
//                          TAG_FALSE (0x7FFC_0000_0000_0003)
//   - string  → allocate StringHeader via js_string_from_bytes,
//               NaN-box with STRING_TAG (0x7FFF)
//   - number  → pass through f64 directly (Perry's NaN-boxing keeps
//               raw f64 numbers as-is)
//   - other   → TAG_UNDEFINED so the closure body's typeof check sees
//               undefined rather than mistyped data
unsafe extern "C" fn invoke_callback1(
    env: *mut NapiEnv,
    info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let mut argc: usize = 2;
    let mut argv: [*mut NapiValue; 2] = [ptr::null_mut(); 2];
    let _ = napi_get_cb_info(
        env,
        info,
        &mut argc,
        argv.as_mut_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
    );
    let mut idx_i32: i32 = -1;
    if argc >= 1 && !argv[0].is_null() {
        let _ = napi_get_value_int32(env, argv[0], &mut idx_i32);
    }
    let arg_d: f64 = if argc >= 2 && !argv[1].is_null() {
        let mut t: c_int = 0;
        let _ = napi_typeof(env, argv[1], &mut t);
        match t {
            // boolean
            2 => {
                let mut b: bool = false;
                let _ = napi_get_value_bool(env, argv[1], &mut b);
                f64::from_bits(if b {
                    0x7FFC_0000_0000_0004
                } else {
                    0x7FFC_0000_0000_0003
                })
            }
            // number
            3 => {
                let mut n: f64 = 0.0;
                let _ = napi_get_value_double(env, argv[1], &mut n);
                n
            }
            // string — read into a stack buffer (4 KB cap), allocate a
            // Perry StringHeader, NaN-box with STRING_TAG. Larger inputs
            // get truncated; future v2.6 follow-up: dynamic alloc via
            // 2-call probe (size first, then bytes).
            4 => {
                let mut buf = [0u8; 4096];
                let mut written: usize = 0;
                let _ = napi_get_value_string_utf8(
                    env,
                    argv[1],
                    buf.as_mut_ptr() as *mut c_char,
                    buf.len(),
                    &mut written,
                );
                let header = crate::string::js_string_from_bytes(buf.as_ptr(), written as u32);
                if header.is_null() {
                    f64::from_bits(0x7FFC_0000_0000_0001)
                } else {
                    crate::value::js_nanbox_string(header as i64)
                }
            }
            _ => f64::from_bits(0x7FFC_0000_0000_0001),
        }
    } else {
        f64::from_bits(0x7FFC_0000_0000_0001)
    };
    crate::arkts_callbacks::arkts_log_napi(&format!(
        "invokeCallback1 NAPI idx={} arg_bits=0x{:016x}",
        idx_i32,
        arg_d.to_bits()
    ));
    if idx_i32 >= 0 {
        let _ = crate::arkts_callbacks::perry_arkts_invoke_callback1(idx_i32 as i64, arg_d);
    }
    let mut undef: *mut NapiValue = ptr::null_mut();
    let _ = napi_get_undefined(env, &mut undef);
    undef
}

// ────────────────────────────────────────────────────────────────────
// perry/media — HarmonyOS AVPlayer drain bridge (issue #369)
// ────────────────────────────────────────────────────────────────────
//
// Same architectural shape as drainToast / drainTextUpdate above. Perry
// can't reach `@ohos.multimedia.media.AVPlayer` directly from a `.so`
// loaded by ArkTS, so:
//
// - TS-side `createPlayer(url)` lowers to `perry_media_create_player`
//   which records a `MediaCreateIntent { handle, url }` in the runtime
//   queue and returns the new handle synchronously.
// - TS-side `play / pause / seek / setVolume / setRate / stop / destroy`
//   each enqueue a `MediaCommand` variant against the existing handle.
// - TS-side `setNowPlaying` enqueues a `MediaNowPlaying` for the
//   AVSession side (best-effort).
//
// ArkTS-side code emitted by `perry-codegen-arkts` polls these three
// drains on a 100 ms tick, dispatches each entry to the matching
// `@ohos.multimedia.media.AVPlayer` op (and `@ohos.multimedia.avsession`
// for now-playing), and pushes state observations back into the runtime
// via `pushMediaState(handle, state, current, duration)`. State queries
// (`getCurrentTime`, `getDuration`, `getState`, `isPlaying`) read from
// the inbox the next push populates.

/// Helper: build a JS string property from a Rust `String` and attach it
/// to `obj` under `key` (NUL-terminated). Caller must ensure `key` ends
/// in a `\0` byte.
unsafe fn napi_set_string_prop(env: *mut NapiEnv, obj: *mut NapiValue, key: &[u8], value: &str) {
    let mut v: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_string_utf8(env, value.as_ptr() as *const c_char, value.len(), &mut v);
    let _ = napi_set_named_property(env, obj, key.as_ptr() as *const c_char, v);
}

/// Helper: attach a `f64`-typed JS number property to `obj` under `key`.
unsafe fn napi_set_number_prop(env: *mut NapiEnv, obj: *mut NapiValue, key: &[u8], value: f64) {
    extern "C" {
        fn napi_create_double(
            env: *mut NapiEnv,
            value: f64,
            result: *mut *mut NapiValue,
        ) -> NapiStatus;
    }
    let mut v: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_double(env, value, &mut v);
    let _ = napi_set_named_property(env, obj, key.as_ptr() as *const c_char, v);
}

/// Pop one queued `createPlayer(url)` request. Returns
/// `{ handle: number, url: string }` or `undefined` when empty. ArkTS
/// loops calling this on tick to allocate AVPlayer instances and wire
/// up their state/time observers.
unsafe extern "C" fn drain_media_create(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let mut intents = crate::media_playback::drain_create_intents();
    if intents.is_empty() {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    }
    // Drain returns the entire queue at once; we hand back exactly one
    // entry per call to keep the ArkTS-side dispatch loop simple. Push
    // the rest back so the next tick picks them up. (Order preserved.)
    let intent = intents.remove(0);
    if !intents.is_empty() {
        crate::media_playback::requeue_create_intents(intents);
    }
    let mut obj: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_object(env, &mut obj);
    napi_set_number_prop(env, obj, b"handle\0", intent.handle as f64);
    napi_set_string_prop(env, obj, b"url\0", &intent.url);
    obj
}

/// Pop one queued control-plane command. Returns
/// `{ op: string, handle: number, ...payload }` or `undefined` when
/// empty. ArkTS dispatches on the `op` string against the matching
/// AVPlayer instance.
///
/// `op` strings: `"play"`, `"pause"`, `"stop"`, `"seek"`,
/// `"setVolume"`, `"setRate"`, `"destroy"`. The `seconds` / `volume` /
/// `rate` fields are present only for the variants that need them.
unsafe extern "C" fn drain_media_control(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let mut cmds = crate::media_playback::drain_control_commands();
    if cmds.is_empty() {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    }
    let cmd = cmds.remove(0);
    if !cmds.is_empty() {
        crate::media_playback::requeue_control_commands(cmds);
    }
    use crate::media_playback::MediaCommand;
    let mut obj: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_object(env, &mut obj);
    let (op, handle, extra_key, extra_val): (&str, i64, Option<&[u8]>, Option<f64>) = match cmd {
        MediaCommand::Play { handle } => ("play", handle, None, None),
        MediaCommand::Pause { handle } => ("pause", handle, None, None),
        MediaCommand::Stop { handle } => ("stop", handle, None, None),
        MediaCommand::Destroy { handle } => ("destroy", handle, None, None),
        MediaCommand::Seek { handle, seconds } => {
            ("seek", handle, Some(b"seconds\0"), Some(seconds))
        }
        MediaCommand::SetVolume { handle, volume } => {
            ("setVolume", handle, Some(b"volume\0"), Some(volume))
        }
        MediaCommand::SetRate { handle, rate } => ("setRate", handle, Some(b"rate\0"), Some(rate)),
    };
    napi_set_string_prop(env, obj, b"op\0", op);
    napi_set_number_prop(env, obj, b"handle\0", handle as f64);
    if let (Some(k), Some(v)) = (extra_key, extra_val) {
        napi_set_number_prop(env, obj, k, v);
    }
    obj
}

/// Pop one queued lock-screen / now-playing metadata update. Returns
/// `{ handle, title, artist, album, artworkUrl }` or `undefined`. ArkTS
/// forwards to `@ohos.multimedia.avsession` (best-effort — AVSession
/// integration may no-op if the user's hap manifest doesn't declare the
/// `ohos.permission.AVSESSION` permission).
unsafe extern "C" fn drain_now_playing(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let mut intents = crate::media_playback::drain_now_playing_intents();
    if intents.is_empty() {
        let mut undef: *mut NapiValue = ptr::null_mut();
        let _ = napi_get_undefined(env, &mut undef);
        return undef;
    }
    let np = intents.remove(0);
    if !intents.is_empty() {
        crate::media_playback::requeue_now_playing_intents(intents);
    }
    let mut obj: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_object(env, &mut obj);
    napi_set_number_prop(env, obj, b"handle\0", np.handle as f64);
    napi_set_string_prop(env, obj, b"title\0", &np.title);
    napi_set_string_prop(env, obj, b"artist\0", &np.artist);
    napi_set_string_prop(env, obj, b"album\0", &np.album);
    napi_set_string_prop(env, obj, b"artworkUrl\0", &np.artwork_url);
    obj
}

/// ArkTS calls this from AVPlayer's `stateChange` / `timeUpdate` handlers
/// to sync the latest state into Perry's runtime, where the synchronous
/// `getState / getCurrentTime / getDuration / isPlaying` accessors read
/// it back. Also fires any registered Perry-side
/// `onStateChange` / `onTimeUpdate` closures (`media_playback::push_media_state`).
///
/// JS signature: `pushMediaState(handle: number, state: string,
/// current: number, duration: number): void`.
unsafe extern "C" fn push_media_state(
    env: *mut NapiEnv,
    info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let mut argc: usize = 4;
    let mut argv: [*mut NapiValue; 4] = [ptr::null_mut(); 4];
    let _ = napi_get_cb_info(
        env,
        info,
        &mut argc,
        argv.as_mut_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
    );

    let handle: i64 = if argc >= 1 && !argv[0].is_null() {
        let mut n: f64 = 0.0;
        let _ = napi_get_value_double(env, argv[0], &mut n);
        n as i64
    } else {
        0
    };

    // Read state string into a stack buffer (32 bytes is plenty — the
    // longest mapped value is "initialized" at 11 bytes).
    let state_str: String = if argc >= 2 && !argv[1].is_null() {
        let mut buf = [0u8; 32];
        let mut written: usize = 0;
        let _ = napi_get_value_string_utf8(
            env,
            argv[1],
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
            &mut written,
        );
        String::from_utf8_lossy(&buf[..written]).into_owned()
    } else {
        String::new()
    };

    let current: f64 = if argc >= 3 && !argv[2].is_null() {
        let mut n: f64 = 0.0;
        let _ = napi_get_value_double(env, argv[2], &mut n);
        n
    } else {
        0.0
    };

    let duration: f64 = if argc >= 4 && !argv[3].is_null() {
        let mut n: f64 = 0.0;
        let _ = napi_get_value_double(env, argv[3], &mut n);
        n
    } else {
        0.0
    };

    let state = crate::media_playback::MediaState::from_avplayer_str(&state_str);
    crate::media_playback::push_media_state(handle, state, current, duration);

    let mut undef: *mut NapiValue = ptr::null_mut();
    let _ = napi_get_undefined(env, &mut undef);
    undef
}

unsafe extern "C" fn napi_init(env: *mut NapiEnv, exports: *mut NapiValue) -> *mut NapiValue {
    // run(): module init + user top-level code. Called from EntryAbility.
    let run_name = b"run\0";
    let mut run_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        run_name.as_ptr() as *const c_char,
        3,
        run,
        ptr::null_mut(),
        &mut run_fn,
    );
    let _ = napi_set_named_property(env, exports, run_name.as_ptr() as *const c_char, run_fn);

    // invokeCallback(idx): dispatch a registered Perry closure by slot.
    // ArkUI's auto-emitted onClick handlers call this with the slot id
    // assigned at compile time by perry-codegen-arkts.
    let cb_name = b"invokeCallback\0";
    let mut cb_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        cb_name.as_ptr() as *const c_char,
        14,
        invoke_callback,
        ptr::null_mut(),
        &mut cb_fn,
    );
    let _ = napi_set_named_property(env, exports, cb_name.as_ptr() as *const c_char, cb_fn);

    // drainToast(): pop one queued toast message and return it as a JS
    // string, or undefined when the queue is empty. Phase 2 v3 Option 1.
    let dt_name = b"drainToast\0";
    let mut dt_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        dt_name.as_ptr() as *const c_char,
        10,
        drain_toast,
        ptr::null_mut(),
        &mut dt_fn,
    );
    let _ = napi_set_named_property(env, exports, dt_name.as_ptr() as *const c_char, dt_fn);

    // drainTextUpdate(): pop one queued (id, value) text update.
    // Phase 2 v3 Option 2.
    let dtu_name = b"drainTextUpdate\0";
    let mut dtu_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        dtu_name.as_ptr() as *const c_char,
        15,
        drain_text_update,
        ptr::null_mut(),
        &mut dtu_fn,
    );
    let _ = napi_set_named_property(env, exports, dtu_name.as_ptr() as *const c_char, dtu_fn);

    // drainVisibilityUpdate(): pop one queued (id, hidden) visibility
    // update. Phase 2 v3.5 — leaf-mutator binding for `widgetSetHidden`.
    let dvu_name = b"drainVisibilityUpdate\0";
    let mut dvu_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        dvu_name.as_ptr() as *const c_char,
        21,
        drain_visibility_update,
        ptr::null_mut(),
        &mut dvu_fn,
    );
    let _ = napi_set_named_property(env, exports, dvu_name.as_ptr() as *const c_char, dvu_fn);

    // drainContentViewUpdate(): pop one queued (target_synth, view_id)
    // content-view update. Phase 2 v3.6 — view-builder lifting.
    let dcv_name = b"drainContentViewUpdate\0";
    let mut dcv_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        dcv_name.as_ptr() as *const c_char,
        22,
        drain_content_view_update,
        ptr::null_mut(),
        &mut dcv_fn,
    );
    let _ = napi_set_named_property(env, exports, dcv_name.as_ptr() as *const c_char, dcv_fn);

    // invokeCallback1(idx, value): dispatch a registered closure with
    // one value arg (Phase 2 v2.5 — Toggle/TextField/Slider onChange).
    let cb1_name = b"invokeCallback1\0";
    let mut cb1_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        cb1_name.as_ptr() as *const c_char,
        15,
        invoke_callback1,
        ptr::null_mut(),
        &mut cb1_fn,
    );
    let _ = napi_set_named_property(env, exports, cb1_name.as_ptr() as *const c_char, cb1_fn);

    // perry/media — issue #369. Four NAPI exports for the AVPlayer
    // drain-bridge: three drain queues (create / control / now-playing)
    // and one state-push entry that ArkTS calls when AVPlayer's
    // stateChange or timeUpdate handlers fire.
    let dmc_name = b"drainMediaCreate\0";
    let mut dmc_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        dmc_name.as_ptr() as *const c_char,
        17,
        drain_media_create,
        ptr::null_mut(),
        &mut dmc_fn,
    );
    let _ = napi_set_named_property(env, exports, dmc_name.as_ptr() as *const c_char, dmc_fn);

    let dmctrl_name = b"drainMediaControl\0";
    let mut dmctrl_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        dmctrl_name.as_ptr() as *const c_char,
        18,
        drain_media_control,
        ptr::null_mut(),
        &mut dmctrl_fn,
    );
    let _ = napi_set_named_property(
        env,
        exports,
        dmctrl_name.as_ptr() as *const c_char,
        dmctrl_fn,
    );

    let dnp_name = b"drainNowPlaying\0";
    let mut dnp_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        dnp_name.as_ptr() as *const c_char,
        16,
        drain_now_playing,
        ptr::null_mut(),
        &mut dnp_fn,
    );
    let _ = napi_set_named_property(env, exports, dnp_name.as_ptr() as *const c_char, dnp_fn);

    let pms_name = b"pushMediaState\0";
    let mut pms_fn: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        pms_name.as_ptr() as *const c_char,
        15,
        push_media_state,
        ptr::null_mut(),
        &mut pms_fn,
    );
    let _ = napi_set_named_property(env, exports, pms_name.as_ptr() as *const c_char, pms_fn);

    exports
}

// OHOS's NativeModuleManager resolves `import X from 'libfoo.so'` by
// stripping `lib`/`.so` from the filename and looking up a module whose
// `nm_modname` equals the result. If they don't match, the import silently
// no-ops and the ArkTS side crashes on first method access with a confusing
// "cannot read property of undefined."
//
// Rather than hardcode a name (which locks users into a specific `-o` flag),
// we derive the modname at load time via `dladdr` on the register function:
// walk back from our own constructor address to the `.so` path, extract the
// filename, strip `lib`/`.so`, copy into a static buffer. Works regardless
// of what the user named their output.

#[repr(C)]
struct DlInfo {
    dli_fname: *const c_char,
    dli_fbase: *mut c_void,
    dli_sname: *const c_char,
    dli_saddr: *mut c_void,
}

extern "C" {
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
    fn strlen(s: *const c_char) -> usize;
}

// 256 bytes is enough for any realistic `.so` filename. Static mut because
// we only write once during .init_array (single-threaded), and it must
// outlive napi_module_register's read of the pointer.
const MODNAME_CAP: usize = 256;
static mut MODNAME_BUF: [u8; MODNAME_CAP] = [0; MODNAME_CAP];

/// Derive modname from the `.so` path reported by dladdr. Strips the
/// leading `lib` and trailing `.so` if present; otherwise uses the
/// filename as-is. Copies into the static buffer and returns a pointer
/// suitable for `nm_modname`. Falls back to "entry" if dladdr fails.
unsafe fn derive_modname() -> *const c_char {
    // Fallback — also what DevEco's hvigor-generated template uses.
    let fallback = b"entry\0";

    let mut info: DlInfo = DlInfo {
        dli_fname: ptr::null(),
        dli_fbase: ptr::null_mut(),
        dli_sname: ptr::null(),
        dli_saddr: ptr::null_mut(),
    };
    let ok = dladdr(derive_modname as *const c_void, &mut info as *mut DlInfo);
    let buf_ptr = &raw mut MODNAME_BUF as *mut u8;
    if ok == 0 || info.dli_fname.is_null() {
        std::ptr::copy_nonoverlapping(fallback.as_ptr(), buf_ptr, fallback.len());
        return buf_ptr as *const c_char;
    }

    // Extract basename: the substring after the last '/'.
    let fname_len = strlen(info.dli_fname);
    let mut base = info.dli_fname;
    let mut probe = info.dli_fname;
    for _ in 0..fname_len {
        if *probe == b'/' as c_char {
            base = probe.add(1);
        }
        probe = probe.add(1);
    }

    // base now points at "libfoo.so" (or whatever). Strip "lib" prefix and
    // ".so" suffix if present.
    let base_len = strlen(base);
    let mut start = base;
    let mut len = base_len;
    if len >= 3 {
        let b0 = *start as u8;
        let b1 = *start.add(1) as u8;
        let b2 = *start.add(2) as u8;
        if b0 == b'l' && b1 == b'i' && b2 == b'b' {
            start = start.add(3);
            len -= 3;
        }
    }
    if len >= 3 {
        let tail = start.add(len - 3);
        let t0 = *tail as u8;
        let t1 = *tail.add(1) as u8;
        let t2 = *tail.add(2) as u8;
        if t0 == b'.' && t1 == b's' && t2 == b'o' {
            len -= 3;
        }
    }

    // Clamp to buffer capacity leaving room for null terminator.
    if len >= MODNAME_CAP {
        len = MODNAME_CAP - 1;
    }

    // Zero the buffer (already zeroed at static init, but reassigning in
    // case of repeated constructor runs — unlikely, but cheap).
    std::ptr::write_bytes(buf_ptr, 0, MODNAME_CAP);
    std::ptr::copy_nonoverlapping(start as *const u8, buf_ptr, len);
    // Null terminator is implicit — buffer is zeroed.

    buf_ptr as *const c_char
}

static mut NAPI_MODULE_DESC: NapiModule = NapiModule {
    nm_version: 1,
    nm_flags: 0,
    nm_filename: ptr::null(),
    nm_register_func: Some(napi_init),
    nm_modname: ptr::null(),
    nm_priv: ptr::null_mut(),
    reserved: [ptr::null_mut(); 4],
};

// Runs on .so load, before any ArkTS code executes.
extern "C" fn register_module() {
    unsafe {
        let desc_ptr = &raw mut NAPI_MODULE_DESC;
        (*desc_ptr).nm_modname = derive_modname();
        napi_module_register(desc_ptr);
    }
}

// The ELF equivalent of `__attribute__((constructor))`. The linker walks
// `.init_array` on `dlopen` and invokes every function pointer.
#[used]
#[link_section = ".init_array"]
static INIT_ARRAY_ENTRY: extern "C" fn() = register_module;
