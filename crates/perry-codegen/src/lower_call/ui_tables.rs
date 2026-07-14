//! perry/ui, perry/system, perry/media, perry/i18n, perry/updater,
//! perry/background, and perry/plugin dispatch table lookups +
//! `lower_perry_ui_table_call` — the unified "lower a perry/* call by
//! looking up its UiSig and emitting the runtime call" machinery.
//!
//! All public-to-the-parent items here are accessed by other siblings
//! (notably `native.rs`) as `super::<name>`; mod.rs explicitly
//! re-exports them under that path.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64};

use perry_dispatch::{
    ArgKind as UiArgKind, MethodRow as UiSig, ReturnKind as UiReturnKind, PERRY_AUDIO_TABLE,
    PERRY_BACKGROUND_TABLE, PERRY_I18N_TABLE, PERRY_MEDIA_TABLE, PERRY_SYSTEM_TABLE,
    PERRY_UI_INSTANCE_TABLE, PERRY_UI_TABLE, PERRY_UPDATER_TABLE,
};

use super::apply_inline_style;

pub fn perry_ui_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UI_TABLE.iter().find(|s| s.method == method)
}

pub fn perry_ui_instance_method_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UI_INSTANCE_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/system dispatch table
// =============================================================================

/// Maps JS import names from `perry/system` to their `perry_system_*` / `perry_*`
/// runtime C symbols. Uses the same UiSig + lower_perry_ui_table_call machinery
/// since the calling convention is identical.

pub fn perry_system_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_SYSTEM_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/media dispatch table
// =============================================================================

/// Maps the TS exports from `types/perry/media/index.d.ts` (createPlayer,
/// play, pause, stop, seek, setVolume, setRate, getCurrentTime, getDuration,
/// getState, isPlaying, onStateChange, onTimeUpdate, setNowPlaying, destroy)
/// to their `perry_media_*` runtime symbols.
pub fn perry_media_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_MEDIA_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/i18n format-wrapper dispatch table
// =============================================================================

/// Maps the TS exports from `types/perry/i18n/index.d.ts` (Currency, Percent,
/// FormatNumber, ShortDate, LongDate, FormatTime, Raw) to their `perry_i18n_*`
/// runtime symbols. Each runtime entry is a default-locale single-arg wrapper
/// over the lower-level `perry_i18n_format_*(value, locale_idx)` exports —
/// the wrapper folds in `LOCALE_INDEX` so the dispatch table here can stay
/// consistent with the other UiSig tables (one TS arg → one runtime arg).
///
/// `t()` is handled separately at the top of `lower_native_method_call`
/// because the perry-transform i18n pass replaces its first arg with an
/// `Expr::I18nString` — there's no runtime call involved.

pub fn perry_i18n_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_I18N_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/updater dispatch table
// =============================================================================

/// Maps the TS exports from `types/perry/updater/index.d.ts` to their runtime
/// symbols exported by the `core` and `desktop` modules of `perry-updater`.
/// The download itself stays in TS (uses existing `fetch()`); this table only
/// covers verify, install, relaunch, sentinel state, and path resolution.
pub fn perry_updater_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UPDATER_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/background dispatch table (issue #538)
// =============================================================================

/// Maps the TS exports from `types/perry/background/index.d.ts` to their
/// runtime symbols (`perry_background_register_task` / `_schedule` /
/// `_cancel`) exported by the per-platform `perry-ui-*` crates.
pub fn perry_background_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_BACKGROUND_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/audio dispatch table (issue #1867)
// =============================================================================

/// Maps the TS exports from `types/perry/audio/index.d.ts` (loadSound, play,
/// stop, pause, resume, setVolume, fadeIn/Out, crossfade, createBus,
/// setBusVolume, …) to their `perry_audio_*` runtime symbols. Backed by
/// AVAudioEngine on Apple, Web Audio API on the WASM target, and (PR 2)
/// miniaudio on Linux / Windows / Android.
pub fn perry_audio_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_AUDIO_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/plugin dispatch table
// =============================================================================

/// Receiver-less (host-side) functions exported from perry/plugin.
/// These map `import { loadPlugin, listPlugins, … } from "perry/plugin"` to
/// their `perry_plugin_*` runtime symbols. Arg shapes match plugin.rs exactly:
/// strings are passed as NaN-boxed f64 (`UiArgKind::F64`) because the runtime
/// calls `extract_string(nanboxed: f64)` internally — not raw pointer.
static PERRY_PLUGIN_TABLE: &[UiSig] = &[
    // loadPlugin(path) -> PluginId (NaN-boxed i64 handle, 0 on failure)
    UiSig {
        method: "loadPlugin",
        runtime: "perry_plugin_load",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::Widget,
    },
    // unloadPlugin(id) -> void
    UiSig {
        method: "unloadPlugin",
        runtime: "perry_plugin_unload",
        args: &[UiArgKind::Widget],
        ret: UiReturnKind::Void,
    },
    // emitHook(hookName, context) -> context (possibly transformed by handlers)
    UiSig {
        method: "emitHook",
        runtime: "perry_plugin_emit_hook",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // emitEvent(event, data) -> undefined
    UiSig {
        method: "emitEvent",
        runtime: "perry_plugin_emit_event",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // invokeTool(name, args) -> handler return value
    UiSig {
        method: "invokeTool",
        runtime: "perry_plugin_invoke_tool",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // setPluginConfig(key, value) -> undefined
    UiSig {
        method: "setPluginConfig",
        runtime: "perry_plugin_set_config",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // discoverPlugins(dir) -> string[] of plugin paths
    UiSig {
        method: "discoverPlugins",
        runtime: "perry_plugin_discover",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // listPlugins() -> { id, name, version, description }[]
    UiSig {
        method: "listPlugins",
        runtime: "perry_plugin_list_plugins",
        args: &[],
        ret: UiReturnKind::F64,
    },
    // listHooks() -> string[]
    UiSig {
        method: "listHooks",
        runtime: "perry_plugin_list_hooks",
        args: &[],
        ret: UiReturnKind::F64,
    },
    // listTools() -> { name, description, pluginId }[]
    UiSig {
        method: "listTools",
        runtime: "perry_plugin_list_tools",
        args: &[],
        ret: UiReturnKind::F64,
    },
    // pluginCount() -> number
    UiSig {
        method: "pluginCount",
        runtime: "perry_plugin_count",
        args: &[],
        ret: UiReturnKind::I64AsF64,
    },
    // initPlugins() -> void  (call once from main before loading plugins)
    UiSig {
        method: "initPlugins",
        runtime: "perry_plugin_init",
        args: &[],
        ret: UiReturnKind::Void,
    },
];

/// Instance methods on a PluginApi handle returned by `loadPlugin`.
/// The handle (NaN-boxed i64) is the receiver and is prepended as the
/// first `i64` arg (`api_handle`) in every runtime call.
static PERRY_PLUGIN_INSTANCE_TABLE: &[UiSig] = &[
    // api.registerHook(hookName, handler) -> undefined
    UiSig {
        method: "registerHook",
        runtime: "perry_plugin_register_hook",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.registerHookEx(hookName, handler, priority, mode) -> undefined
    UiSig {
        method: "registerHookEx",
        runtime: "perry_plugin_register_hook_ex",
        args: &[
            UiArgKind::F64,
            UiArgKind::Closure,
            UiArgKind::I64Raw,
            UiArgKind::I64Raw,
        ],
        ret: UiReturnKind::F64,
    },
    // api.registerTool(name, description, handler) -> undefined
    UiSig {
        method: "registerTool",
        runtime: "perry_plugin_register_tool",
        args: &[UiArgKind::F64, UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.registerService(name, startFn, stopFn) -> undefined
    UiSig {
        method: "registerService",
        runtime: "perry_plugin_register_service",
        args: &[UiArgKind::F64, UiArgKind::Closure, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.registerRoute(path, handler) -> undefined
    UiSig {
        method: "registerRoute",
        runtime: "perry_plugin_register_route",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.getConfig(key) -> any
    UiSig {
        method: "getConfig",
        runtime: "perry_plugin_get_config",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.log(level, message) -> undefined   (level: 0=DEBUG,1=INFO,2=WARN,3=ERROR)
    UiSig {
        method: "log",
        runtime: "perry_plugin_log",
        args: &[UiArgKind::I64Raw, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.setMetadata(name, version, description) -> undefined
    UiSig {
        method: "setMetadata",
        runtime: "perry_plugin_set_metadata",
        args: &[UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.on(event, handler) -> undefined
    UiSig {
        method: "on",
        runtime: "perry_plugin_on",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.emit(event, data) -> undefined
    UiSig {
        method: "emit",
        runtime: "perry_plugin_emit",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.unregisterHook(hookName, handler) -> undefined
    // Removes the single entry whose closure bits match `handler`. The
    // caller must be the same plugin that registered the hook; otherwise
    // the call is a silent no-op.
    UiSig {
        method: "unregisterHook",
        runtime: "perry_plugin_unregister_hook",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.unregisterTool(name) -> undefined
    UiSig {
        method: "unregisterTool",
        runtime: "perry_plugin_unregister_tool",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.unregisterService(name) -> undefined
    // The service's `stopFn` is invoked before the entry is removed,
    // matching the lifecycle contract of `registerService`.
    UiSig {
        method: "unregisterService",
        runtime: "perry_plugin_unregister_service",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.unregisterRoute(path) -> undefined
    UiSig {
        method: "unregisterRoute",
        runtime: "perry_plugin_unregister_route",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.off(event, handler) -> undefined
    // Removes the single event-bus subscription whose closure bits match.
    UiSig {
        method: "off",
        runtime: "perry_plugin_off",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
];

pub fn perry_plugin_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_PLUGIN_TABLE.iter().find(|s| s.method == method)
}

pub fn perry_plugin_instance_method_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_PLUGIN_INSTANCE_TABLE
        .iter()
        .find(|s| s.method == method)
}

/// Lower a perry/ui call described by `sig`. Walks each arg, applies
/// the per-kind coercion to produce an LLVM SSA value of the right type,
/// lazy-declares the runtime function, emits the call, and boxes the
/// return value per `sig.ret`.
///
/// Arity handling (issue #6087). A row's `args` is the *runtime ABI*, which
/// is wider than the TypeScript surface whenever the `.d.ts` marks a trailing
/// param optional (`shareText(text, title?)`, `Image(url, alt?)`,
/// `stop(handle, fadeOutMs?)`). Three shapes are accepted:
///
/// * exactly `sig.args.len()` args — the common case;
/// * one *extra* trailing arg on a `Widget`-returning constructor — absorbed
///   as an inline style object (issue #185, see `apply_inline_style`);
/// * *fewer* args, where every omitted trailing param is a `Str`/`F64` — the
///   TS-optional case. Those are padded with the empty value (`""` / `0`) and
///   the call is emitted for real.
///
/// Anything else is a hard compile error. This branch used to lower the args
/// for their side effects and return the `0.0` sentinel — i.e. **silently drop
/// the call**. That silence is what turned the `perry/system` name-collision
/// bug (#6087) into a silent miscompile instead of a diagnostic, and it also
/// meant a legitimate-but-under-supplied `Image(url)` compiled to no widget at
/// all. A dropped call must never be the answer to an arity we don't
/// understand.
pub fn lower_perry_ui_table_call(
    ctx: &mut FnCtx<'_>,
    sig: &UiSig,
    args: &[Expr],
) -> Result<String> {
    // Issue #185 Phase C step 4: when a Widget-returning constructor is
    // called with one extra trailing arg, treat it as an inline `style`
    // object and apply via `apply_inline_style` after the create call.
    // Lets every widget in the table (Text, Toggle, Slider, TextField,
    // Spacer, Divider, ImageFile, ImageSymbol, ProgressView, NavStack,
    // ZStack, etc.) accept the same React-style ergonomics that Button
    // already has, with no per-widget code edits.
    // Issue #389: `appSetTimer` accepts both `(intervalMs, callback)`
    // (the user-facing 2-arg form per the type stub) and
    // `(app, intervalMs, callback)` (the historical 3-arg form). The
    // dispatch table declares 3 args (`Widget, F64, Closure`); the
    // platform runtime helpers all ignore `_app_handle`. When the
    // user supplies only 2 args, prepend a synthetic 0 Widget so the
    // call still matches the 3-arg ABI without changing the runtime
    // signatures across 8 platform crates.
    let synthesised_args: Vec<Expr>;
    let args: &[Expr] = if sig.method == "appSetTimer" && args.len() == 2 && sig.args.len() == 3 {
        synthesised_args = std::iter::once(Expr::Integer(0))
            .chain(args.iter().cloned())
            .collect();
        &synthesised_args[..]
    } else if sig.method == "appSetActivationPolicy" && args.len() == 1 && sig.args.len() == 2 {
        // User-facing `appSetActivationPolicy(policy)` → 2-arg ABI
        // `(app_handle, policy)`; synthesise the ignored 0 app-handle,
        // mirroring the `appSetTimer` adapter above.
        synthesised_args = std::iter::once(Expr::Integer(0))
            .chain(args.iter().cloned())
            .collect();
        &synthesised_args[..]
    } else if sig.method == "drawImage" && sig.args.len() == 9 {
        // Canvas.drawImage mirrors HTML Canvas' three overloads, but the
        // native FFI surface uses one fixed ABI:
        //   (image, sx, sy, sw, sh, dx, dy, dw, dh)
        // A negative source/dest size is the runtime sentinel for intrinsic
        // image dimensions.
        synthesised_args = match args.len() {
            3 => vec![
                args[0].clone(),
                Expr::Integer(0),
                Expr::Integer(0),
                Expr::Integer(-1),
                Expr::Integer(-1),
                args[1].clone(),
                args[2].clone(),
                Expr::Integer(-1),
                Expr::Integer(-1),
            ],
            5 => vec![
                args[0].clone(),
                Expr::Integer(0),
                Expr::Integer(0),
                Expr::Integer(-1),
                Expr::Integer(-1),
                args[1].clone(),
                args[2].clone(),
                args[3].clone(),
                args[4].clone(),
            ],
            9 => args.to_vec(),
            _ => Vec::new(),
        };
        if synthesised_args.is_empty() {
            args
        } else {
            &synthesised_args[..]
        }
    } else {
        args
    };

    // Issue #6087: pad TS-optional trailing params so the call is actually
    // emitted. The dispatch row always spells the full runtime ABI, but the
    // `.d.ts` lets the caller omit a trailing `title?` / `alt?` / `fadeMs?`.
    // Pre-fix those calls hit the arity mismatch below and were dropped
    // wholesale — `shareText("hi")` and `Image(url)` compiled to nothing.
    // Only value-shaped kinds have a meaningful empty value; a missing
    // `Widget` handle or `Closure` can't be synthesized, so those fall
    // through to the hard error.
    let padded_args: Vec<Expr>;
    let args: &[Expr] = if args.len() < sig.args.len()
        && sig.args[args.len()..]
            .iter()
            .all(|k| matches!(k, UiArgKind::Str | UiArgKind::F64))
    {
        let mut padded: Vec<Expr> = args.to_vec();
        for kind in &sig.args[args.len()..] {
            padded.push(match kind {
                UiArgKind::Str => Expr::String(String::new()),
                _ => Expr::Integer(0),
            });
        }
        padded_args = padded;
        &padded_args[..]
    } else {
        args
    };

    let inline_style_arg: Option<&Expr> =
        if args.len() == sig.args.len() + 1 && matches!(sig.ret, UiReturnKind::Widget) {
            Some(&args[sig.args.len()])
        } else {
            None
        };
    let declared_arg_count = sig.args.len();

    if args.len() != declared_arg_count && inline_style_arg.is_none() {
        // Issue #6087: a `perry/*` builtin called with an arity the runtime
        // ABI cannot accept. Everything legitimate has been absorbed above
        // (arg adapters, inline style, optional trailing params), so this is
        // a genuine mismatch — fail the compile instead of quietly emitting
        // no call at all, which is what let #6087 hide for so long.
        anyhow::bail!(
            "perry/* builtin `{}` takes {} argument(s), but it was called with {}.\n  \
             note: it lowers to the native runtime symbol `{}`, whose ABI is fixed — \
             the call cannot be emitted with this arity.",
            sig.method,
            declared_arg_count,
            args.len(),
            sig.runtime,
        );
    }

    // Lower each arg according to its declared kind. Build two parallel
    // vectors so we can pass them through to `blk.call(...)` in one shot
    // without intermediate borrows. Iterate the declared sig args only
    // — the inline-style trailing arg (if present) is consumed below.
    let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
        Vec::with_capacity(declared_arg_count);
    let mut runtime_param_types: Vec<crate::types::LlvmType> =
        Vec::with_capacity(declared_arg_count);
    for (kind, arg) in sig.args.iter().zip(args.iter().take(declared_arg_count)) {
        match kind {
            UiArgKind::Widget => {
                // Widgets are NaN-boxed pointers. Lower as JSValue,
                // strip the POINTER_TAG bits to get the raw 1-based
                // handle as i64.
                let v = lower_expr(ctx, arg)?;
                let blk = ctx.block();
                let h = unbox_to_i64(blk, &v);
                llvm_args.push((I64, h));
                runtime_param_types.push(I64);
            }
            UiArgKind::Str => {
                let h = super::get_raw_string_ptr(ctx, arg)?;
                llvm_args.push((I64, h));
                runtime_param_types.push(I64);
            }
            UiArgKind::F64 => {
                let v = lower_expr(ctx, arg)?;
                llvm_args.push((DOUBLE, v));
                runtime_param_types.push(DOUBLE);
            }
            UiArgKind::Closure => {
                // Closures are NaN-boxed pointers passed as f64. The
                // runtime side calls `js_closure_call0` (or callN) on
                // them, so it expects the f64 representation.
                let v = lower_expr(ctx, arg)?;
                llvm_args.push((DOUBLE, v));
                runtime_param_types.push(DOUBLE);
            }
            UiArgKind::I64Raw => {
                // Numeric arg the runtime wants as i64 (e.g. enum tag,
                // boolean flag). `fptosi` converts the f64 to a signed
                // integer.
                let v = lower_expr(ctx, arg)?;
                let blk = ctx.block();
                let i = blk.fptosi(DOUBLE, &v, I64);
                llvm_args.push((I64, i));
                runtime_param_types.push(I64);
            }
        }
    }

    // Lazy-declare the runtime function so the linker pulls in the
    // libperry_ui_*.a symbol. Same pending_declares mechanism the
    // cross-module call site uses for `perry_fn_*`.
    let return_type = match sig.ret {
        UiReturnKind::Widget | UiReturnKind::Promise | UiReturnKind::I64AsF64 => I64,
        UiReturnKind::F64 => DOUBLE,
        UiReturnKind::Void => crate::types::VOID,
        UiReturnKind::Str => I64,
    };
    ctx.pending_declares
        .push((sig.runtime.to_string(), return_type, runtime_param_types));

    // Emit the call. Slices need a borrow of `llvm_args` because the
    // tuple's second field is `String` and `blk.call` expects `&str`.
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
    match sig.ret {
        UiReturnKind::Widget | UiReturnKind::Promise => {
            // Scope `blk` so the mutable borrow on `ctx` is released
            // before the optional `apply_inline_style` call re-borrows.
            let handle = {
                let blk = ctx.block();
                blk.call(I64, sig.runtime, &arg_slices)
            };
            // Issue #185 Phase C step 4: apply inline style if a
            // trailing object literal was passed. Promise-returning helpers
            // are not widgets and must not receive styles.
            if sig.ret == UiReturnKind::Widget {
                if let Some(style_arg) = inline_style_arg {
                    apply_inline_style(ctx, &handle, style_arg)?;
                }
            }
            let blk = ctx.block();
            Ok(nanbox_pointer_inline(blk, &handle))
        }
        UiReturnKind::F64 => Ok(ctx.block().call(DOUBLE, sig.runtime, &arg_slices)),
        UiReturnKind::Void => {
            ctx.block().call_void(sig.runtime, &arg_slices);
            Ok(double_literal(0.0))
        }
        UiReturnKind::Str => {
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            Ok(nanbox_string_inline(blk, &raw))
        }
        UiReturnKind::I64AsF64 => {
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            Ok(blk.sitofp(I64, &raw, DOUBLE))
        }
    }
}
