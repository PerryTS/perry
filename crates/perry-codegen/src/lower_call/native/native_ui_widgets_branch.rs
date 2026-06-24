{
    // perry/ui Text(content, id) — 2-arg form registers the widget in the
    // per-platform text registry so setText(id, val) can update it later.
    // The 1-arg form `Text(content)` routes through the PERRY_UI_TABLE entry
    // (perry_ui_text_create) as normal; only the 2-arg form is intercepted here.
    if module == "perry/ui" && method == "Text" && object.is_none() && args.len() == 2 {
        let text_ptr = get_raw_string_ptr(ctx, &args[0])?;
        let id_ptr = get_raw_string_ptr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_ui_text_create_with_id".to_string(),
            I64,
            vec![I64, I64],
        ));
        let blk = ctx.block();
        let handle = blk.call(
            I64,
            "perry_ui_text_create_with_id",
            &[(I64, &text_ptr), (I64, &id_ptr)],
        );
        // Optional trailing style arg (position 2) — same pattern as Button.
        if let Some(style_arg) = args.get(2).cloned() {
            apply_inline_style(ctx, &handle, &style_arg)?;
        }
        let blk = ctx.block();
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    // perry/ui Button — TS shape is `Button(label, handler)` where
    // handler is a closure. The simple positional form is what mango
    // uses. The Object-config form (`Button(label, { onPress: cb })`)
    // is a followup.
    if module == "perry/ui" && method == "Button" && object.is_none() {
        let label_ptr = if let Some(label) = args.first() {
            get_raw_string_ptr(ctx, label)?
        } else {
            "0".to_string()
        };
        let handler_d = if let Some(handler) = args.get(1) {
            lower_expr(ctx, handler)?
        } else {
            "0.0".to_string()
        };
        ctx.pending_declares
            .push(("perry_ui_button_create".to_string(), I64, vec![I64, DOUBLE]));
        // Scope `blk` so the mutable borrow on `ctx` is released before
        // we call `apply_inline_style(ctx, ...)`, which re-borrows.
        let handle = {
            let blk = ctx.block();
            blk.call(
                I64,
                "perry_ui_button_create",
                &[(I64, &label_ptr), (DOUBLE, &handler_d)],
            )
        };

        // Issue #185 Phase C step 2: optional trailing `style` arg.
        // `Button(label, onPress, { borderRadius, opacity, ... })`
        // destructures the StyleProps object at HIR time and emits a
        // sequence of setter calls against the just-created handle.
        // Mirrors the v0.5.x `App({ title, width, height, body })` HIR
        // pass — same `extract_options_fields` helper, same per-key
        // routing. Step 2 covers single-value scalar props; colors /
        // padding / shadow / gradient need multi-arg destructure and
        // land in step 3.
        if let Some(style_arg) = args.get(2) {
            apply_inline_style(ctx, &handle, style_arg)?;
        }

        let blk = ctx.block();
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    // Generic perry/ui receiver-less dispatch via a per-method table.
    // Constructors and setters that don't need special arg shape handling
    // (object literals, children arrays, closures stored in side tables)
    // route through here. Each entry declares the runtime function name
    // plus the arg coercion + return boxing rules.
    //
    // The table covers ~80% of mango's perry/ui surface. Special cases
    // (App with object literal, VStack/HStack with children array,
    // Button with optional Object config) are handled in dedicated
    // arms BELOW so they short-circuit before this table is consulted.
    //
    // Extending: add a row to PERRY_UI_TABLE matching the TS method name
    // to the perry_ui_* runtime function and arg shape. Most setters
    // follow `(widget, …number args)` and most constructors return a
    // widget handle that gets NaN-boxed as POINTER on the way out.
    // perry/ui.showToast(msg) — Phase 2 v3 Option 1. Enqueues `msg`
    // into the runtime's drain queue; the auto-emitted .ets onClick
    // pumps the queue into ArkUI's `promptAction.showToast` after the
    // closure body returns. On non-harmonyos targets the runtime FFI
    // is still defined (just with empty queue + no consumer) so
    // cross-platform code compiles, but only harmonyos shows visual
    // feedback. Future v3 follow-up: route to NSAlert/UIAlertController/
    // GtkPopover on the desktop UI backends.
    // perry/ui.onFrame(cb) — one-shot display-link callback. Issue #1865.
    // The callback fires once on the next vsync with (timestampMs, deltaMs).
    // Idiomatic loop: re-register from inside the callback.
    if module == "perry/ui" && method == "onFrame" && object.is_none() {
        if args.len() != 1 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let cb_box = lower_expr(ctx, &args[0])?;
        ctx.pending_declares
            .push(("js_on_frame_callback".to_string(), I64, vec![I64]));
        let blk = ctx.block();
        let cb_handle = unbox_to_i64(blk, &cb_box);
        let id = blk.call(I64, "js_on_frame_callback", &[(I64, &cb_handle)]);
        return Ok(nanbox_pointer_inline(ctx.block(), &id));
    }

    // perry/ui.cancelFrame(id) — cancel a pending onFrame registration.
    // Accepts the pointer-tagged handle returned by `onFrame`.
    if module == "perry/ui" && method == "cancelFrame" && object.is_none() {
        if args.len() != 1 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let id_box = lower_expr(ctx, &args[0])?;
        ctx.pending_declares
            .push(("js_cancel_frame".to_string(), crate::types::VOID, vec![I64]));
        let blk = ctx.block();
        let id_handle = unbox_to_i64(blk, &id_box);
        blk.call_void("js_cancel_frame", &[(I64, &id_handle)]);
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    if module == "perry/ui" && method == "showToast" && object.is_none() {
        if args.is_empty() {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let msg_d = lower_expr(ctx, &args[0])?;
        ctx.pending_declares.push((
            "perry_arkts_show_toast".to_string(),
            crate::types::VOID,
            vec![DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void("perry_arkts_show_toast", &[(DOUBLE, &msg_d)]);
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // perry/ui.setText(id, value) — Phase 2 v3 Option 2 reactive Text.
    // Enqueues a (id, value) update; the auto-emitted .ets onClick
    // pumps the queue into the matching `@State text_<id>` after the
    // closure body returns. Same drain-pattern shape as showToast.
    if module == "perry/ui" && method == "setText" && object.is_none() {
        if args.len() < 2 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let id_d = lower_expr(ctx, &args[0])?;
        let val_d = lower_expr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_arkts_set_text".to_string(),
            crate::types::VOID,
            vec![DOUBLE, DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void("perry_arkts_set_text", &[(DOUBLE, &id_d), (DOUBLE, &val_d)]);
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // Issue #535 — perry/ui `state<T>` desugar trio. Synthetic methods
    // emitted only by `crates/perry-transform/src/state_desugar.rs`.
    if module == "perry/ui"
        && (method == "__state_init" || method == "__state_set")
        && object.is_none()
    {
        if args.len() != 2 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let id_d = lower_expr(ctx, &args[0])?;
        let val_d = lower_expr(ctx, &args[1])?;
        let runtime_fn = if method == "__state_init" {
            "js_state_init"
        } else {
            "js_state_set"
        };
        ctx.pending_declares.push((
            runtime_fn.to_string(),
            crate::types::VOID,
            vec![DOUBLE, DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void(runtime_fn, &[(DOUBLE, &id_d), (DOUBLE, &val_d)]);
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }
    if module == "perry/ui" && method == "__state_get" && object.is_none() {
        if args.len() != 1 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let id_d = lower_expr(ctx, &args[0])?;
        ctx.pending_declares
            .push(("js_state_get".to_string(), DOUBLE, vec![DOUBLE]));
        let blk = ctx.block();
        let result = blk.call(DOUBLE, "js_state_get", &[(DOUBLE, &id_d)]);
        return Ok(result);
    }

    // Issue #610 — `__foreach_register(synth_id, host, render_closure)`
    // synthetic method emitted by state_desugar's `ForEach(stateBinding,
    // render)` rewrite. Forwards (synth_id, host_handle, render_closure)
    // to the runtime registry. The runtime walks this map on every
    // js_state_set for the matching synth id, calling the platform's
    // foreach-render handler with the new count value — the platform
    // crate (perry-ui-macos / perry-ui-gtk4 / etc.) clears the host's
    // children, calls render_closure(i) for each i in [0..count), and
    // adds each returned widget.
    if module == "perry/ui" && method == "__foreach_register" && object.is_none() {
        if args.len() != 3 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let synth_id_d = lower_expr(ctx, &args[0])?;
        let host_d = lower_expr(ctx, &args[1])?;
        let host_i64 = unbox_to_i64(ctx.block(), &host_d);
        let render_d = lower_expr(ctx, &args[2])?;
        ctx.pending_declares.push((
            "js_foreach_register".to_string(),
            crate::types::VOID,
            vec![DOUBLE, I64, DOUBLE],
        ));
        ctx.block().call_void(
            "js_foreach_register",
            &[(DOUBLE, &synth_id_d), (I64, &host_i64), (DOUBLE, &render_d)],
        );
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // Issue #535 Layer 2 — `__navstack_register_route(synth_id, name, body)`
    // synthetic method emitted by state_desugar's NavStack(state, routes)
    // rewrite. Lowers `body` to a widget handle (NaN-boxed pointer →
    // unbox to i64) and forwards (synth_id, name, handle) to the runtime
    // registry. The runtime walks this map on every js_state_set for the
    // matching synth id, toggling each route's NSView.isHidden via the
    // platform handler registered by perry-ui-macos at app startup.
    if module == "perry/ui" && method == "__navstack_register_route" && object.is_none() {
        if args.len() != 3 {
            return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
        }
        let synth_id_d = lower_expr(ctx, &args[0])?;
        let name_d = lower_expr(ctx, &args[1])?;
        let body_d = lower_expr(ctx, &args[2])?;
        let body_i64 = unbox_to_i64(ctx.block(), &body_d);
        ctx.pending_declares.push((
            "js_navstack_register_route".to_string(),
            crate::types::VOID,
            vec![DOUBLE, DOUBLE, I64],
        ));
        ctx.block().call_void(
            "js_navstack_register_route",
            &[(DOUBLE, &synth_id_d), (DOUBLE, &name_d), (I64, &body_i64)],
        );
        // Return the body handle (already NaN-boxed) so the rewrite can
        // chain by binding the result as the route's host child.
        return Ok(body_d);
    }

    // perry/arkts: HarmonyOS Phase 2 v2 callback bridge. Synthetic module
    // injected by the harvest pass (`compile.rs::emit_index_ets`) — never
    // user-authored. `registerCallback(idx, closure)` lowers to a call to
    // the runtime FFI `perry_arkts_register_callback(i64, f64)` which
    // stores the closure pointer in a slot table that NAPI's
    // `invokeCallback(idx)` dispatches against on ArkUI tap events.
    if module == "perry/arkts" && method == "registerCallback" && object.is_none() {
        if args.len() != 2 {
            bail!(
                "perry/arkts.registerCallback expects (idx, closure), got {} args",
                args.len()
            );
        }
        let idx_d = lower_expr(ctx, &args[0])?;
        let closure_d = lower_expr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_arkts_register_callback".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        let blk = ctx.block();
        let idx_i64 = blk.fptosi(DOUBLE, &idx_d, I64);
        blk.call_void(
            "perry_arkts_register_callback",
            &[(I64, &idx_i64), (DOUBLE, &closure_d)],
        );
        return Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)));
    }

    // perry/system dispatch: audioStart, audioGetLevel, getDeviceModel, etc.
    if module == "perry/system" && object.is_none() {
        if method == "notificationSchedule" {
            return lower_notification_schedule(ctx, args);
        }
        if args.is_empty() {
            match method {
                "getAppVersion" => {
                    let version = ctx.app_metadata.version.clone();
                    let idx = ctx.strings.intern(&version);
                    let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
                    return Ok(ctx.block().load(DOUBLE, &handle_global));
                }
                "getAppBuildNumber" => {
                    return Ok(double_literal(ctx.app_metadata.build_number as f64));
                }
                "getBundleId" => {
                    let bundle_id = ctx.app_metadata.bundle_id.clone();
                    let idx = ctx.strings.intern(&bundle_id);
                    let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
                    return Ok(ctx.block().load(DOUBLE, &handle_global));
                }
                _ => {}
            }
        }
        if let Some(sig) = perry_system_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
    }

    // perry/audio dispatch (issue #1867): loadSound, play, stop, pause,
    // setVolume, fadeIn/Out, crossfade, createBus, setBusVolume, …
    // Low-latency game-engine-style audio backed by AVAudioEngine on
    // Apple, Web Audio API on WASM, and (PR 2) miniaudio on Linux /
    // Windows / Android. Distinct from perry/media (streaming + UI).
    if module == "perry/audio" && object.is_none() {
        if let Some(sig) = perry_audio_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        bail!(
            "perry/audio: '{}' is not a known function (args: {}). \
             Check types/perry/audio/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    // perry/media dispatch: createPlayer, play, pause, seek, setVolume,
    // onStateChange, onTimeUpdate, setNowPlaying, destroy. Streaming
    // media playback backed by AVPlayer (Apple), MediaPlayer/JNI
    // (Android), GStreamer (GTK4/Linux), Media Foundation (Windows).
    if module == "perry/media" && object.is_none() {
        if let Some(sig) = perry_media_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        bail!(
            "perry/media: '{}' is not a known function (args: {}). \
             Check types/perry/media/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    // perry/i18n format wrappers: Currency, Percent, FormatNumber, ShortDate,
    // LongDate, FormatTime, Raw. Without this, the call falls through to the
    // receiver-less early-out and returns NaN-boxed `undefined` (issue #188).
    // `t()` is dispatched separately near the top of this function.
    if module == "perry/i18n" && object.is_none() {
        if let Some(sig) = perry_i18n_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
    }

    // perry/plugin dispatch: loadPlugin, listPlugins, emitHook, etc.
    if module == "perry/plugin" && object.is_none() {
        if let Some(sig) = perry_plugin_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        bail!(
            "perry/plugin: '{}' is not a known function (args: {}). \
             Check types/perry/plugin/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    // perry/updater dispatch: compareVersions, verifyHash, verifySignature,
    // sentinel state helpers, install, relaunch.
    if module == "perry/updater" && object.is_none() {
        if let Some(sig) = perry_updater_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        bail!(
            "perry/updater: '{}' is not a known function (args: {}). \
             Check types/perry/updater/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    // Phase 2 v3.3: `Text(content, id)` reactive form. The 1-arg
    // `Text(content)` row in PERRY_UI_TABLE doesn't know about the
    // optional `id` second arg — pre-fix the table-call's "if args.len()
    // == sig.args.len() + 1 ⇒ inline_style_arg" path absorbed it as a
    // would-be style object, then `apply_inline_style` silently no-op'd
    // because strings aren't object literals. Effect: id was dropped on
    // the floor and `setText("counter", ...)` had nothing to look up.
    //
    // Fix: detect Text-with-id BEFORE the table lookup, lower the
    // create call manually (mirroring the table-call shape), then
    // emit `perry_arkts_register_text_id(handle, id)` so the platform
    // UI lib can map id → widget handle. On harmonyos, codegen-arkts
    // emits `@State text_<id>` directly into the .ets and the
    // register_text_id call is a runtime no-op (see
    // perry-runtime/src/ui_text_registry.rs).
    if module == "perry/ui" && method == "Text" && object.is_none() && args.len() == 2 {
        let content_ptr = get_raw_string_ptr(ctx, &args[0])?;
        ctx.pending_declares
            .push(("perry_ui_text_create".to_string(), I64, vec![I64]));
        let handle = {
            let blk = ctx.block();
            blk.call(I64, "perry_ui_text_create", &[(I64, &content_ptr)])
        };
        // Lower the id arg as a regular NaN-boxed JS value so the
        // runtime's `decode_jsvalue_string` can read it through the
        // standard StringHeader path (handles SSO + heap strings the
        // same way, and matches the harmonyos drain-queue contract).
        let id_d = lower_expr(ctx, &args[1])?;
        ctx.pending_declares.push((
            "perry_arkts_register_text_id".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        let blk = ctx.block();
        blk.call_void(
            "perry_arkts_register_text_id",
            &[(I64, &handle), (DOUBLE, &id_d)],
        );
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    if module == "perry/ui"
        && object.is_none()
        && method != "App"
        && method != "VStack"
        && method != "HStack"
        // Image + WebView have option-bag handlers further down that
        // do their own arg destructuring; they're not in perry_ui_table
        // so they must skip this catch-all bail.
        && method != "Image"
        && method != "WebView"
    {
        if let Some(sig) = perry_ui_table_lookup(method) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        // Fail fast at compile time so a missing/misspelled method
        // surfaces as an error instead of silently returning 0.0 —
        // which used to compile, link, and run with a zero widget
        // handle (no window, or null-pointer crash at the caller).
        bail!(
            "perry/ui: '{}' is not a known function (args: {}). \
             Check the spelling and consult types/perry/ui/index.d.ts \
             for the supported API surface.",
            method,
            args.len()
        );
    }
}
