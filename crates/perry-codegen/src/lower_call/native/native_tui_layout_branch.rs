{
    // `perry/ui.App({ title, width, height, body, icon? })` — minimum-viable
    // dispatch so a perry/ui app actually launches an NSApplication and
    // shows a window. Pre-v0.5.10 this fell into the receiver-less early-
    // out below and returned `double 0.0`, so the program completed
    // without entering the AppKit run loop — mango compiled cleanly but
    // exited immediately on launch with no output. This is the smallest
    // dispatch that proves the linking + runtime + Mach-O code path works
    // end to end. Other perry/ui constructors (Text, Button, VStack,
    // HStack, etc.) are NOT dispatched yet so the body is the
    // zero-sentinel — the window appears with the right title/size but
    // no widget tree. Full widget dispatch is a separate followup.
    // perry/tui Text(content, { fg, bg, bold, italic, underline, reverse }) —
    // the second-arg options form for #405 Phase 3.5 styling. Dispatches to
    // `js_perry_tui_text_styled` with the four-color/style args; the bare
    // 1-arg `Text(content)` form keeps falling through to the regular
    // PERRY_UI_TABLE dispatch which routes to `js_perry_tui_text`. Object
    // literals reach this point as `Expr::New { class_name: __AnonShape_… }`
    // — use `extract_options_fields` to pull the fields out either way.
    if module == "perry/tui" && method == "Text" && object.is_none() && args.len() >= 2 {
        if let Some(props) = extract_options_fields(ctx, &args[1]) {
            let content_ptr = get_raw_string_ptr(ctx, &args[0])?;
            let mut fg_str = Expr::String(String::new());
            let mut bg_str = Expr::String(String::new());
            let mut style_bits: u8 = 0;
            for (key, val) in &props {
                match key.as_str() {
                    "fg" | "color" => fg_str = val.clone(),
                    "bg" | "backgroundColor" => bg_str = val.clone(),
                    "bold" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0001;
                        }
                    }
                    "italic" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0010;
                        }
                    }
                    "underline" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0100;
                        }
                    }
                    // ink uses "inverse"; #358 used "reverse". Accept both.
                    "reverse" | "inverse" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0000_1000;
                        }
                    }
                    // ink-shape parity (#679 Phase 5): dimColor + strikethrough.
                    "dimColor" | "dim" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0001_0000;
                        }
                    }
                    "strikethrough" => {
                        if matches!(val, Expr::Bool(true)) {
                            style_bits |= 0b0010_0000;
                        }
                    }
                    _ => {}
                }
            }
            let fg_ptr = get_raw_string_ptr(ctx, &fg_str)?;
            let bg_ptr = get_raw_string_ptr(ctx, &bg_str)?;
            let bits_lit = double_literal(style_bits as f64);
            ctx.pending_declares.push((
                "js_perry_tui_text_styled".to_string(),
                I64,
                vec![I64, I64, I64, DOUBLE],
            ));
            let handle = ctx.block().call(
                I64,
                "js_perry_tui_text_styled",
                &[
                    (I64, &content_ptr),
                    (I64, &fg_ptr),
                    (I64, &bg_ptr),
                    (DOUBLE, &bits_lit),
                ],
            );
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    }

    // perry/tui Input(value, cursor) — 2-arg form for arbitrary-position
    // cursor. The runtime decomposes into a row Box of [before, cursor,
    // after] Text widgets so the cursor character draws with reverse
    // video at the right offset. The 1-arg `Input(value)` form falls
    // through to the regular dispatch table. (#404.)
    if module == "perry/tui" && method == "Input" && object.is_none() && args.len() >= 2 {
        let content_ptr = get_raw_string_ptr(ctx, &args[0])?;
        let cursor = lower_expr(ctx, &args[1])?;
        ctx.pending_declares
            .push(("js_perry_tui_input_at".to_string(), I64, vec![I64, DOUBLE]));
        let handle = ctx.block().call(
            I64,
            "js_perry_tui_input_at",
            &[(I64, &content_ptr), (DOUBLE, &cursor)],
        );
        return Ok(nanbox_pointer_inline(ctx.block(), &handle));
    }

    // perry/tui AnimatedSpinner({ interval, frames }) — unpacks the
    // options object and dispatches to `js_perry_tui_animated_spinner`.
    // Both opts are optional; the runtime falls back to 100 ms /
    // ['-', '\\', '|', '/']. Handles 0-arg, 1-arg-options, and 1-arg-
    // non-options (treated as default) call shapes here so bare
    // `AnimatedSpinner()` doesn't trip over the dispatch table's
    // 2-arg arity expectation. (#403.)
    if module == "perry/tui" && method == "AnimatedSpinner" && object.is_none() {
        let mut interval_expr: Expr = Expr::Number(0.0);
        let mut frames_expr: Option<Expr> = None;
        if let Some(first) = args.first() {
            if let Some(props) = extract_options_fields(ctx, first) {
                for (k, v) in &props {
                    match k.as_str() {
                        "interval" => interval_expr = v.clone(),
                        "frames" => frames_expr = Some(v.clone()),
                        _ => {}
                    }
                }
            }
        }
        let interval = lower_expr(ctx, &interval_expr)?;
        let frames = match frames_expr {
            Some(e) => lower_expr(ctx, &e)?,
            None => double_literal(0.0),
        };
        let frames_h = unbox_to_i64(ctx.block(), &frames);
        ctx.pending_declares.push((
            "js_perry_tui_animated_spinner".to_string(),
            I64,
            vec![DOUBLE, I64],
        ));
        let handle = ctx.block().call(
            I64,
            "js_perry_tui_animated_spinner",
            &[(DOUBLE, &interval), (I64, &frames_h)],
        );
        return Ok(nanbox_pointer_inline(ctx.block(), &handle));
    }

    // perry/tui Table({ headers, rows, selected }) — unpacks the options
    // object and dispatches to `js_perry_tui_table(headers_ptr, rows_ptr,
    // selected_idx)`. The 2D `rows` array is passed through unchanged;
    // the runtime walks it via `read_string_2d_array`. (#402.)
    if module == "perry/tui" && method == "Table" && object.is_none() && !args.is_empty() {
        if let Some(props) = extract_options_fields(ctx, &args[0]) {
            let mut headers_expr: Option<Expr> = None;
            let mut rows_expr: Option<Expr> = None;
            let mut selected_expr: Expr = Expr::Number(-1.0);
            for (k, v) in &props {
                match k.as_str() {
                    "headers" => headers_expr = Some(v.clone()),
                    "rows" => rows_expr = Some(v.clone()),
                    "selected" => selected_expr = v.clone(),
                    _ => {}
                }
            }
            let headers = match headers_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let rows = match rows_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let selected = lower_expr(ctx, &selected_expr)?;
            // Unbox the array pointers (NaN-boxed POINTER) into raw i64.
            let blk = ctx.block();
            let headers_h = unbox_to_i64(blk, &headers);
            let rows_h = unbox_to_i64(blk, &rows);
            ctx.pending_declares.push((
                "js_perry_tui_table".to_string(),
                I64,
                vec![I64, I64, DOUBLE],
            ));
            let handle = ctx.block().call(
                I64,
                "js_perry_tui_table",
                &[(I64, &headers_h), (I64, &rows_h), (DOUBLE, &selected)],
            );
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    }

    // perry/tui Tabs({ tabs, active, body }) — unpacks the options
    // object and dispatches to `js_perry_tui_tabs(tabs_ptr, active,
    // body_ptr)`. `body` is an array of widget handles; only the
    // active tab's body is mounted. (#402.)
    if module == "perry/tui" && method == "Tabs" && object.is_none() && !args.is_empty() {
        if let Some(props) = extract_options_fields(ctx, &args[0]) {
            let mut tabs_expr: Option<Expr> = None;
            let mut active_expr: Expr = Expr::Number(0.0);
            let mut body_expr: Option<Expr> = None;
            for (k, v) in &props {
                match k.as_str() {
                    "tabs" => tabs_expr = Some(v.clone()),
                    "active" => active_expr = v.clone(),
                    "body" => body_expr = Some(v.clone()),
                    _ => {}
                }
            }
            let tabs = match tabs_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let active = lower_expr(ctx, &active_expr)?;
            let body = match body_expr {
                Some(e) => lower_expr(ctx, &e)?,
                None => double_literal(0.0),
            };
            let blk = ctx.block();
            let tabs_h = unbox_to_i64(blk, &tabs);
            let body_h = unbox_to_i64(blk, &body);
            ctx.pending_declares.push((
                "js_perry_tui_tabs".to_string(),
                I64,
                vec![I64, DOUBLE, I64],
            ));
            let handle = ctx.block().call(
                I64,
                "js_perry_tui_tabs",
                &[(I64, &tabs_h), (DOUBLE, &active), (I64, &body_h)],
            );
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    }

    // perry/tui Box — TS shapes:
    //   Box()                                — empty container
    //   Box([child, …])                      — children array (Phase 1)
    //   Box({ flexDirection, gap, … }, [child, …])  — style + children (Phase 3)
    //   Box({ flexDirection, gap, … })       — style, no children
    //
    // Detect which by examining args[0]: an array → children-only;
    // an object/object-shape → style; followed by an array → children.
    // Mirrors the perry/ui VStack pattern: create handle, optionally
    // emit per-style-field setter calls, then iterate the children
    // array calling add_child per element. Bare `Box()` falls through
    // to the regular PERRY_UI_TABLE dispatch (just emits js_perry_tui_box).
    // (#358 Phases 1 + 3.)
    if module == "perry/tui" && method == "Box" && object.is_none() && !args.is_empty() {
        // Note: js_perry_tui_box returns I64 (raw handle); the
        // dispatch table's NR_PTR contract NaN-boxes it for the
        // outer call. The special-case path here mirrors that — call
        // returns I64, store in an I64 slot, NaN-box at the very end
        // when handing off to the caller.
        ctx.pending_declares
            .push(("js_perry_tui_box".to_string(), I64, vec![]));
        ctx.pending_declares.push((
            "js_perry_tui_box_add_child".to_string(),
            DOUBLE,
            vec![I64, I64],
        ));
        let blk = ctx.block();
        let parent_handle = blk.call(I64, "js_perry_tui_box", &[]);
        let parent_slot = ctx.func.alloca_entry(I64);
        ctx.block().store(I64, &parent_handle, &parent_slot);

        // Determine which arg is the style-options object and which
        // is the children array.
        //
        // 2-arg shape `Box(opts, children)` — first is always style,
        // second is always children, regardless of whether `children`
        // is a literal array or a runtime value like `msgs.map(...)`.
        // The old structural classifier only recognised `Expr::Array`
        // as children, so `Box(opts, runtimeArr)` silently dropped the
        // children. (#679 follow-up.)
        //
        // 1-arg shape: classify structurally — an Object-shaped
        // expression is style, anything else is children.
        let mut style_arg: Option<&Expr> = None;
        let mut children_arg: Option<&Expr> = None;
        if args.len() >= 2 {
            style_arg = Some(&args[0]);
            children_arg = Some(&args[1]);
        } else if let Some(arg) = args.first() {
            match arg {
                Expr::Array(_) | Expr::ArraySpread(_) => children_arg = Some(arg),
                Expr::Object(_) | Expr::New { .. } => style_arg = Some(arg),
                // Bare identifier / call / etc. — most TS programs
                // use this for children, e.g. `Box(rows)` where
                // `rows = messages.map(…)`. Treat as children.
                _ => children_arg = Some(arg),
            }
        }

        // Emit per-field style setter calls if a style object was
        // recognized. Each known field maps to one js_perry_tui_box_set_*
        // FFI; unknown fields are silently dropped (forward-compat
        // for future style props).
        if let Some(style) = style_arg {
            apply_box_style(ctx, &parent_slot, style)?;
        }

        if let Some(children_expr) = children_arg {
            let elements_owned: Option<Vec<Expr>> = match children_expr {
                Expr::Array(elems) => Some(elems.clone()),
                _ => None,
            };
            if let Some(elements) = elements_owned {
                for child in &elements {
                    let child_box = lower_expr(ctx, child)?;
                    let blk = ctx.block();
                    let child_handle = unbox_to_i64(blk, &child_box);
                    let parent_reload = blk.load(I64, &parent_slot);
                    blk.call_void(
                        "js_perry_tui_box_add_child",
                        &[(I64, &parent_reload), (I64, &child_handle)],
                    );
                }
            } else {
                // Non-literal children (e.g. `Box(messages.map(m => Text(m)))`)
                // — lower to a runtime array pointer + delegate iteration
                // to `js_perry_tui_box_add_children_array`. Pre-#679-follow-up
                // this branch dropped the result and the Box ended up empty.
                let children_box = lower_expr(ctx, children_expr)?;
                let blk = ctx.block();
                let children_handle = unbox_to_i64(blk, &children_box);
                ctx.pending_declares.push((
                    "js_perry_tui_box_add_children_array".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                let blk = ctx.block();
                let parent_reload = blk.load(I64, &parent_slot);
                blk.call(
                    DOUBLE,
                    "js_perry_tui_box_add_children_array",
                    &[(I64, &parent_reload), (I64, &children_handle)],
                );
            }
        }

        let blk = ctx.block();
        let parent_final = blk.load(I64, &parent_slot);
        // NaN-box the handle into a POINTER-tagged f64 — same as the
        // dispatch table's NR_PTR contract.
        return Ok(nanbox_pointer_inline(blk, &parent_final));
    }

    // perry/ui VStack/HStack — special-case because the TS shape is
    // `VStack(spacing, [child1, child2, ...])` (or just `VStack([...])`),
    // but the runtime takes only `(spacing) -> handle` and children get
    // added one by one via `perry_ui_widget_add_child`. We can't express
    // this with the per-method table because it's variadic in arg shape
    // *and* needs sequential calls per child.
    if module == "perry/ui" && (method == "VStack" || method == "HStack") && object.is_none() {
        let runtime_create = if method == "VStack" {
            "perry_ui_vstack_create"
        } else {
            "perry_ui_hstack_create"
        };
        // First arg may be the spacing number OR the children array
        // (when the user calls `VStack([children])` without an explicit
        // spacing). Detect which by checking the type.
        let (spacing_d, children_idx) = match args.first() {
            Some(Expr::Array(_)) | Some(Expr::ArraySpread(_)) => ("8.0".to_string(), 0),
            Some(other) => {
                // Could be a number (spacing) — lower it. The children
                // are then in args[1] (if present).
                let v = lower_expr(ctx, other)?;
                (v, 1)
            }
            None => ("8.0".to_string(), 0),
        };
        ctx.pending_declares
            .push((runtime_create.to_string(), I64, vec![DOUBLE]));
        let blk = ctx.block();
        let parent_handle = blk.call(I64, runtime_create, &[(DOUBLE, &spacing_d)]);
        // Stash so add_child has it; we'll need to reload later because
        // calls between here and the loop may invalidate `parent_handle`'s
        // SSA name in subsequent blocks.
        let parent_slot = ctx.func.alloca_entry(I64);
        ctx.block().store(I64, &parent_handle, &parent_slot);

        // Walk the children array (if present). For each element, lower
        // to a JSValue, unbox to widget handle, call
        // `perry_ui_widget_add_child(parent, child)`.
        ctx.pending_declares.push((
            "perry_ui_widget_add_child".to_string(),
            crate::types::VOID,
            vec![I64, I64],
        ));
        if let Some(children_expr) = args.get(children_idx) {
            let elements_owned: Option<Vec<Expr>> = match children_expr {
                Expr::Array(elems) => Some(elems.clone()),
                _ => None,
            };
            if let Some(elements) = elements_owned {
                for child in &elements {
                    let child_box = lower_expr(ctx, child)?;
                    let blk = ctx.block();
                    let child_handle = unbox_to_i64(blk, &child_box);
                    let parent_reload = blk.load(I64, &parent_slot);
                    blk.call_void(
                        "perry_ui_widget_add_child",
                        &[(I64, &parent_reload), (I64, &child_handle)],
                    );
                }
            } else {
                // Children expression isn't a literal array — emit an
                // inline LLVM loop that walks the runtime array and calls
                // `perry_ui_widget_add_child` for each element. Without
                // this, `for (const x of xs) ys.push(chip(x));
                // HStack(8, ys)` and similar patterns silently dropped
                // every loop-built widget (#634); only the literal-array
                // shape produced render output.
                let arr_d = lower_expr(ctx, children_expr)?;
                let arr_ptr = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &arr_d)
                };
                ctx.pending_declares
                    .push(("js_array_get_length".to_string(), I64, vec![I64]));
                let len = ctx
                    .block()
                    .call(I64, "js_array_get_length", &[(I64, &arr_ptr)]);

                let i_slot = ctx.func.alloca_entry(I64);
                ctx.block().store(I64, "0", &i_slot);

                let header_idx = ctx.new_block("ui_addch.header");
                let body_idx = ctx.new_block("ui_addch.body");
                let exit_idx = ctx.new_block("ui_addch.exit");
                let header_label = ctx.block_label(header_idx);
                let body_label = ctx.block_label(body_idx);
                let exit_label = ctx.block_label(exit_idx);
                ctx.block().br(&header_label);

                ctx.current_block = header_idx;
                let i_h = ctx.block().load(I64, &i_slot);
                let cmp = ctx.block().icmp_slt(I64, &i_h, &len);
                ctx.block().cond_br(&cmp, &body_label, &exit_label);

                ctx.current_block = body_idx;
                ctx.pending_declares.push((
                    "js_array_get_element".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                let i_b = ctx.block().load(I64, &i_slot);
                let elem_d = ctx.block().call(
                    DOUBLE,
                    "js_array_get_element",
                    &[(I64, &arr_ptr), (I64, &i_b)],
                );
                let child_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &elem_d)
                };
                let parent_reload = ctx.block().load(I64, &parent_slot);
                ctx.block().call_void(
                    "perry_ui_widget_add_child",
                    &[(I64, &parent_reload), (I64, &child_handle)],
                );
                let one_l = "1".to_string();
                let i_next = ctx.block().add(I64, &i_b, &one_l);
                ctx.block().store(I64, &i_next, &i_slot);
                ctx.block().br(&header_label);

                ctx.current_block = exit_idx;
            }
        }

        // Issue #185 Phase C step 5: optional inline `style: { ... }`
        // arg AFTER the children array. Position depends on whether
        // spacing was passed first:
        //   VStack(children, style?)              children_idx=0, style at args[1]
        //   VStack(spacing, children, style?)     children_idx=1, style at args[2]
        // `apply_inline_style` no-ops on non-object trailing args, so
        // the call is safe even when it's accidentally something else.
        let style_idx = children_idx + 1;
        if let Some(style_arg) = args.get(style_idx).cloned() {
            let parent_handle_str = ctx.block().load(I64, &parent_slot);
            apply_inline_style(ctx, &parent_handle_str, &style_arg)?;
        }

        let blk = ctx.block();
        let parent_final = blk.load(I64, &parent_slot);
        return Ok(nanbox_pointer_inline(blk, &parent_final));
    }

    // perry/ui ForEach — TS shape is `ForEach(state, (i) => Widget)`. The
    // runtime's `perry_ui_for_each_init` wants `(container, state, closure)`,
    // so we synthesize a VStack container, call for_each_init with it, and
    // return the container handle. Without this special case the call falls
    // through to the generic dispatch which emits the "method 'ForEach' not
    // in dispatch table" warning and returns 0/undefined — the outer VStack
    // then tries to add_child with an invalid handle, AppKit silently fails
    // to attach the window body, and the process runs but no window shows.
    if module == "perry/ui" && method == "ForEach" && object.is_none() && args.len() == 2 {
        ctx.pending_declares
            .push(("perry_ui_vstack_create".to_string(), I64, vec![DOUBLE]));
        ctx.pending_declares.push((
            "perry_ui_for_each_init".to_string(),
            crate::types::VOID,
            vec![I64, I64, DOUBLE],
        ));

        let spacing = "8.0".to_string();
        let blk = ctx.block();
        let container = blk.call(I64, "perry_ui_vstack_create", &[(DOUBLE, &spacing)]);
        let container_slot = ctx.func.alloca_entry(I64);
        ctx.block().store(I64, &container, &container_slot);

        // args[0]: State handle — NaN-boxed pointer, unbox to i64.
        let state_box = lower_expr(ctx, &args[0])?;
        let blk = ctx.block();
        let state_handle = unbox_to_i64(blk, &state_box);

        // args[1]: render closure — stays as a NaN-boxed f64.
        let closure_d = lower_expr(ctx, &args[1])?;

        let blk = ctx.block();
        let container_reload = blk.load(I64, &container_slot);
        blk.call_void(
            "perry_ui_for_each_init",
            &[
                (I64, &container_reload),
                (I64, &state_handle),
                (DOUBLE, &closure_d),
            ],
        );

        let blk = ctx.block();
        let container_final = blk.load(I64, &container_slot);
        return Ok(nanbox_pointer_inline(blk, &container_final));
    }
}
