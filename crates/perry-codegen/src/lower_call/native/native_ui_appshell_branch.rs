{
    // perry/ui Image({ url, alt? }) — issue #635. The positional form
    // `Image(url, alt?)` is picked up by the perry_ui table below; the
    // object-literal form is destructured here into the same call shape
    // by extracting the `url` and `alt` fields and forwarding to the
    // table. Anything else on the object (placeholder / contentMode in
    // the documented surface) is silently dropped — those fields are
    // post-v1.
    if module == "perry/ui" && method == "Image" && object.is_none() && args.len() == 1 {
        if let Some(props) = extract_options_fields(ctx, &args[0]) {
            let mut url_arg: Option<Expr> = None;
            let mut alt_arg: Option<Expr> = None;
            let mut system_name_arg: Option<Expr> = None;
            for (key, val) in &props {
                match key.as_str() {
                    "url" => url_arg = Some(val.clone()),
                    "alt" => alt_arg = Some(val.clone()),
                    // #1495: Image({ systemName }) -> SF-symbol image,
                    // routed to the same runtime as ImageSymbol(name).
                    "systemName" => system_name_arg = Some(val.clone()),
                    _ => {
                        // Lower for side effects so any nested closures
                        // are still collected.
                        let _ = lower_expr(ctx, val)?;
                    }
                }
            }
            if let Some(name) = system_name_arg {
                if let Some(sig) = perry_ui_table_lookup("ImageSymbol") {
                    return lower_perry_ui_table_call(ctx, sig, &[name]);
                }
            }
            if let Some(u) = url_arg {
                let positional = vec![u, alt_arg.unwrap_or_else(|| Expr::String(String::new()))];
                if let Some(sig) = perry_ui_table_lookup("Image") {
                    return lower_perry_ui_table_call(ctx, sig, &positional);
                }
            }
        }
    }

    // perry/ui WebView({ url, allowedDomains?, userAgent?, ephemeral?,
    //                    onShouldNavigate?, onLoaded?, onError?,
    //                    width?, height? }) — issue #658 Phase 1.
    //
    // Single object-literal form. Codegen calls
    // `perry_ui_webview_create(url, w, h)` then for every other present
    // key emits a corresponding `perry_ui_webview_set_*` call against
    // the returned handle. Same shape as the App({...}) destructure
    // above. There's no positional `WebView(url, w, h)` overload —
    // option-bag is the only TS surface (every parameter is optional
    // except url, and named is much more readable for ~9 fields).
    if module == "perry/ui" && method == "WebView" && object.is_none() && args.len() == 1 {
        let Some(props) = extract_options_fields(ctx, &args[0]) else {
            bail!(
                "perry/ui: WebView(...) requires a config object literal. Use \
                 `WebView({{ url: ..., onShouldNavigate: (u) => ..., onLoaded: (u) => ... }})` \
                 (see types/perry/ui/index.d.ts)."
            );
        };

        let mut url_ptr: String = "0".to_string();
        let mut width_d: String = "0.0".to_string();
        let mut height_d: String = "0.0".to_string();
        let mut user_agent_ptr: Option<String> = None;
        let mut allowed_domains_handle: Option<String> = None;
        let mut ephemeral_d: Option<String> = None;
        let mut on_should_navigate_d: Option<String> = None;
        let mut on_loaded_d: Option<String> = None;
        let mut on_error_d: Option<String> = None;

        for (key, val) in &props {
            match key.as_str() {
                "url" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    url_ptr = unbox_to_i64(blk, &v);
                }
                "width" => {
                    width_d = lower_expr(ctx, val)?;
                }
                "height" => {
                    height_d = lower_expr(ctx, val)?;
                }
                "userAgent" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    user_agent_ptr = Some(unbox_to_i64(blk, &v));
                }
                "allowedDomains" => {
                    // The user passes a JS array of strings; we treat it as a
                    // generic widget-like handle (i64 unbox of POINTER) and
                    // the runtime walks it via js_array_get_length / element.
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    allowed_domains_handle = Some(unbox_to_i64(blk, &v));
                }
                "ephemeral" => {
                    // Boolean → JS truthy → f64 → i64 (1 = ephemeral).
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    let truthy = blk.call(I64, "js_is_truthy", &[(DOUBLE, &v)]);
                    ephemeral_d = Some(truthy);
                }
                "onShouldNavigate" => {
                    on_should_navigate_d = Some(lower_expr(ctx, val)?);
                }
                "onLoaded" => {
                    on_loaded_d = Some(lower_expr(ctx, val)?);
                }
                "onError" => {
                    on_error_d = Some(lower_expr(ctx, val)?);
                }
                _ => {
                    // Unknown key — lower for side effects so any nested
                    // closures still get collected by the closure-conversion
                    // pass.
                    let _ = lower_expr(ctx, val)?;
                }
            }
        }

        ctx.pending_declares.push((
            "perry_ui_webview_create".to_string(),
            I64,
            // v2-B: 4th arg is `ephemeral_hint` (1.0 ephemeral / 0.0 persistent).
            vec![I64, DOUBLE, DOUBLE, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_user_agent".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_allowed_domains".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_ephemeral".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_on_should_navigate".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_on_loaded".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_webview_set_on_error".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares
            .push(("js_is_truthy".to_string(), I64, vec![DOUBLE]));

        // v2-B: pass ephemeral as a creation-time arg so backends with
        // construction-time data-store choices (WebView2 userDataFolder,
        // WebKitGTK NetworkSession::new_ephemeral) honor it before the
        // first navigation. Default 1.0 = ephemeral when the user omits
        // the field. The truthy lowering above produces an i64 (0 / 1);
        // bitcast to a double via sitofp so the FFI sees an f64 hint.
        let blk = ctx.block();
        let eph_hint = if let Some(eph) = &ephemeral_d {
            blk.sitofp(I64, eph, DOUBLE)
        } else {
            double_literal(1.0)
        };

        let handle = blk.call(
            I64,
            "perry_ui_webview_create",
            &[
                (I64, &url_ptr),
                (DOUBLE, &width_d),
                (DOUBLE, &height_d),
                (DOUBLE, &eph_hint),
            ],
        );
        if let Some(ua) = &user_agent_ptr {
            blk.call_void(
                "perry_ui_webview_set_user_agent",
                &[(I64, &handle), (I64, ua)],
            );
        }
        if let Some(dom) = &allowed_domains_handle {
            blk.call_void(
                "perry_ui_webview_set_allowed_domains",
                &[(I64, &handle), (I64, dom)],
            );
        }
        if let Some(cb) = &on_should_navigate_d {
            blk.call_void(
                "perry_ui_webview_set_on_should_navigate",
                &[(I64, &handle), (DOUBLE, cb)],
            );
        }
        if let Some(cb) = &on_loaded_d {
            blk.call_void(
                "perry_ui_webview_set_on_loaded",
                &[(I64, &handle), (DOUBLE, cb)],
            );
        }
        if let Some(cb) = &on_error_d {
            blk.call_void(
                "perry_ui_webview_set_on_error",
                &[(I64, &handle), (DOUBLE, cb)],
            );
        }

        // Return as a NaN-boxed widget handle (POINTER tag).
        return Ok(nanbox_pointer_inline(blk, &handle));
    }

    if module == "perry/ui" && method == "App" && object.is_none() {
        if args.len() != 1 {
            bail!(
                "perry/ui: App(...) takes a single config object literal like \
                 `App({{ title, width, height, body }})`, got {} argument(s). \
                 There is no `App(title, builder)` callback form.",
                args.len()
            );
        }
        let Some(props) = extract_options_fields(ctx, &args[0]) else {
            bail!(
                "perry/ui: App(...) requires a config object literal. Use \
                 `App({{ title: ..., width: ..., height: ..., body: ... }})` \
                 (see types/perry/ui/index.d.ts)."
            );
        };
        let mut title_ptr: String = "0".to_string();
        let mut width_d: String = "1024.0".to_string();
        let mut height_d: String = "768.0".to_string();
        let mut body_handle: String = "0".to_string();
        let mut icon_ptr: Option<String> = None;
        let mut window_state_ptr: Option<String> = None;
        let mut frameless_val: Option<String> = None;
        let mut level_ptr: Option<String> = None;
        let mut transparent_val: Option<String> = None;
        let mut vibrancy_ptr: Option<String> = None;
        let mut activation_policy_ptr: Option<String> = None;
        for (key, val) in &props {
            match key.as_str() {
                "title" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    title_ptr = unbox_to_i64(blk, &v);
                }
                "width" => {
                    width_d = lower_expr(ctx, val)?;
                }
                "height" => {
                    height_d = lower_expr(ctx, val)?;
                }
                "body" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    body_handle = unbox_to_i64(blk, &v);
                }
                "icon" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    icon_ptr = Some(unbox_to_i64(blk, &v));
                }
                // Issue #1280 — `windowState: "normal" | "maximized" | "fullscreen"`.
                // Forwarded to perry_ui_app_set_window_state; each platform
                // backend applies the state at app_run time.
                "windowState" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    window_state_ptr = Some(unbox_to_i64(blk, &v));
                }
                // v0.4.11 launcher-style window options, lost in the Phase K
                // Cranelift→LLVM cutover (2026-07-16 docs audit). `frameless`
                // and `transparent` are forwarded as the raw NaN-boxed value —
                // every platform backend only acts when the bits equal
                // TAG_TRUE, exactly like the original Cranelift wiring.
                "frameless" => {
                    frameless_val = Some(lower_expr(ctx, val)?);
                }
                "transparent" => {
                    transparent_val = Some(lower_expr(ctx, val)?);
                }
                // `level` / `vibrancy` / `activationPolicy` are strings. Route
                // through the SSO-safe unbox (js_get_string_pointer_unified)
                // rather than the raw pointer mask: short literals like
                // "modal" or "menu" can arrive as inline SSO values whose low
                // 48 bits are not a StringHeader pointer.
                "level" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    level_ptr = Some(crate::expr::unbox_str_handle(blk, &v));
                }
                "vibrancy" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    vibrancy_ptr = Some(crate::expr::unbox_str_handle(blk, &v));
                }
                "activationPolicy" => {
                    let v = lower_expr(ctx, val)?;
                    let blk = ctx.block();
                    activation_policy_ptr = Some(crate::expr::unbox_str_handle(blk, &v));
                }
                _ => {
                    let _ = lower_expr(ctx, val)?;
                }
            }
        }
        ctx.pending_declares.push((
            "perry_ui_app_create".to_string(),
            I64,
            vec![I64, DOUBLE, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_icon".to_string(),
            crate::types::VOID,
            vec![I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_window_state".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_frameless".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_level".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_transparent".to_string(),
            crate::types::VOID,
            vec![I64, DOUBLE],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_vibrancy".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_activation_policy".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_set_body".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        ctx.pending_declares.push((
            "perry_ui_app_run".to_string(),
            crate::types::VOID,
            vec![I64],
        ));
        let blk = ctx.block();
        let app_handle = blk.call(
            I64,
            "perry_ui_app_create",
            &[(I64, &title_ptr), (DOUBLE, &width_d), (DOUBLE, &height_d)],
        );
        if let Some(icon) = icon_ptr {
            blk.call_void("perry_ui_app_set_icon", &[(I64, &icon)]);
        }
        if let Some(state_ptr) = window_state_ptr {
            blk.call_void(
                "perry_ui_app_set_window_state",
                &[(I64, &app_handle), (I64, &state_ptr)],
            );
        }
        // Window properties are applied BEFORE the body so vibrancy /
        // frameless can reconfigure the window (content view swap, style
        // mask) before Auto Layout constraints are installed — same call
        // order the Cranelift backend used (v0.4.11).
        if let Some(v) = &frameless_val {
            blk.call_void(
                "perry_ui_app_set_frameless",
                &[(I64, &app_handle), (DOUBLE, v)],
            );
        }
        if let Some(p) = &level_ptr {
            blk.call_void("perry_ui_app_set_level", &[(I64, &app_handle), (I64, p)]);
        }
        if let Some(v) = &transparent_val {
            blk.call_void(
                "perry_ui_app_set_transparent",
                &[(I64, &app_handle), (DOUBLE, v)],
            );
        }
        if let Some(p) = &vibrancy_ptr {
            blk.call_void("perry_ui_app_set_vibrancy", &[(I64, &app_handle), (I64, p)]);
        }
        if let Some(p) = &activation_policy_ptr {
            blk.call_void(
                "perry_ui_app_set_activation_policy",
                &[(I64, &app_handle), (I64, p)],
            );
        }
        blk.call_void(
            "perry_ui_app_set_body",
            &[(I64, &app_handle), (I64, &body_handle)],
        );
        blk.call_void("perry_ui_app_run", &[(I64, &app_handle)]);
        return Ok(double_literal(0.0));
    }
}
