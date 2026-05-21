use super::*;

pub(super) const TUI_ROWS: &[NativeModSig] = &[
    // ========== perry/tui (#358 Phase 1) ==========
    // Text(content) and Box() return widget handles (NaN-boxed POINTER).
    // The Box(children: Widget[]) shape is intercepted earlier in
    // lower_call/native.rs and lowered as Box() + add_child*N; this
    // table only matches the bare-arg shapes.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Text",
        class_filter: None,
        runtime: "js_perry_tui_text",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Box",
        class_filter: None,
        runtime: "js_perry_tui_box",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "render",
        class_filter: None,
        runtime: "js_perry_tui_render",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "enter",
        class_filter: None,
        runtime: "js_perry_tui_enter",
        args: &[],
        ret: NR_VOID,
    },
    // perry/tui Phase 2 — state container, useInput, run loop, exit.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "state",
        class_filter: None,
        runtime: "js_perry_tui_state_alloc",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // state.get() — receiver call, dispatches against class "State"
    // registered by destructuring.rs.
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "get",
        class_filter: Some("State"),
        runtime: "js_perry_tui_state_get",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "set",
        class_filter: Some("State"),
        runtime: "js_perry_tui_state_set",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useInput",
        class_filter: None,
        runtime: "js_perry_tui_use_input",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "run",
        class_filter: None,
        runtime: "js_perry_tui_run",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "exit",
        class_filter: None,
        runtime: "js_perry_tui_exit",
        args: &[],
        ret: NR_VOID,
    },
    // perry/tui Phase 3 — Box style setters. The codegen at
    // lower_call/native.rs intercepts `Box(opts, children)` and emits
    // these explicitly per style field; they're not normally called
    // directly from user code but are listed here so the dispatch
    // table also handles direct hand-emission cases (e.g. a future
    // `box.setFlexDirection(...)` imperative API).
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexDirection",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_direction",
        args: &[NA_PTR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetJustifyContent",
        class_filter: None,
        runtime: "js_perry_tui_box_set_justify_content",
        args: &[NA_PTR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetAlignItems",
        class_filter: None,
        runtime: "js_perry_tui_box_set_align_items",
        args: &[NA_PTR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetGap",
        class_filter: None,
        runtime: "js_perry_tui_box_set_gap",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetPadding",
        class_filter: None,
        runtime: "js_perry_tui_box_set_padding",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetWidth",
        class_filter: None,
        runtime: "js_perry_tui_box_set_width",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetHeight",
        class_filter: None,
        runtime: "js_perry_tui_box_set_height",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexGrow",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_grow",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    // perry/tui Phase 3.5 — per-side padding, flex-shrink/basis,
    // percentage units. (#405.) Codegen-emitted from the Box-options
    // path; not normally called directly from user code.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetPaddingEach",
        class_filter: None,
        runtime: "js_perry_tui_box_set_padding_each",
        args: &[NA_PTR, NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexShrink",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_shrink",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexBasis",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_basis",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexBasisPct",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_basis_pct",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetWidthPct",
        class_filter: None,
        runtime: "js_perry_tui_box_set_width_pct",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetHeightPct",
        class_filter: None,
        runtime: "js_perry_tui_box_set_height_pct",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "TextStyled",
        class_filter: None,
        runtime: "js_perry_tui_text_styled",
        args: &[NA_STR, NA_STR, NA_STR, NA_F64],
        ret: NR_PTR,
    },
    // perry/tui Phase 4 — Spacer + ProgressBar widgets.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Spacer",
        class_filter: None,
        runtime: "js_perry_tui_spacer",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "ProgressBar",
        class_filter: None,
        runtime: "js_perry_tui_progress_bar",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.5 — Spinner / Input / List / Select / TextArea.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Spinner",
        class_filter: None,
        runtime: "js_perry_tui_spinner",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Input",
        class_filter: None,
        runtime: "js_perry_tui_input",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "List",
        class_filter: None,
        runtime: "js_perry_tui_list",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Select",
        class_filter: None,
        runtime: "js_perry_tui_select",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "TextArea",
        class_filter: None,
        runtime: "js_perry_tui_text_area",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.6 — Table + Tabs widgets. Direct-FFI shapes
    // (positional args); object-literal `Table({headers, rows, selected})`
    // is unpacked at the codegen level (lower_call/native.rs).
    // (#402.)
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Table",
        class_filter: None,
        runtime: "js_perry_tui_table",
        args: &[NA_PTR, NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Tabs",
        class_filter: None,
        runtime: "js_perry_tui_tabs",
        args: &[NA_PTR, NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.7 — Input(value, cursor). Direct-call shape;
    // codegen also dispatches to this from the 2-arg form so the
    // table acts as a fallback for hand-emitted calls. (#404.)
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "InputAt",
        class_filter: None,
        runtime: "js_perry_tui_input_at",
        args: &[NA_STR, NA_F64],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.7 — AnimatedSpinner. Bare `AnimatedSpinner()`
    // hits this row with both args defaulted; object-literal opts
    // form is unpacked at the codegen level. (#403.)
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "AnimatedSpinner",
        class_filter: None,
        runtime: "js_perry_tui_animated_spinner",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    // ========== perry/tui Phase 1 — ink-API ergonomics hooks (#679) ==========
    // useState(initial) — call-site-indexed state cell. Returns the
    // current value (initialised to `initial` on the first call). Pair
    // with useStateSet(slot_idx, v) to write. Slot index === hook
    // index seen by this useState call (matches React rule-of-hooks).
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useState",
        class_filter: None,
        runtime: "js_perry_tui_use_state",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // useStateSet(slot_idx, value) — write to a useState slot + flip
    // STATE_DIRTY when the bits change.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useStateSet",
        class_filter: None,
        runtime: "js_perry_tui_use_state_set",
        args: &[NA_F64, NA_F64],
        ret: NR_VOID,
    },
    // useStateTuple(initial) — returns a [value, setter] array. This
    // is the back-end the destructuring rewriter (destructuring.rs:
    // rewrite_use_state_tuple) emits when user code writes
    // `const [v, setV] = useState(initial)`. NR_PTR so the returned
    // array handle gets POINTER-tagged like a normal Perry array.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useStateTuple",
        class_filter: None,
        runtime: "js_perry_tui_use_state_tuple",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // useEffect(fn, deps?). Runs fn() on first call or when deps change.
    // fn is an unboxed closure pointer (NA_PTR); deps is an unboxed
    // array pointer (NA_PTR) or 0 for "no deps array → run every render".
    // The runtime hashes the deps array elements bit-identity-style.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useEffect",
        class_filter: None,
        runtime: "js_perry_tui_use_effect",
        args: &[NA_PTR, NA_PTR],
        ret: NR_VOID,
    },
    // useMemo(fn, deps) — same deps convention; runs fn and caches
    // the result. Returns the cached value when deps haven't changed.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useMemo",
        class_filter: None,
        runtime: "js_perry_tui_use_memo",
        args: &[NA_PTR, NA_PTR],
        ret: NR_F64,
    },
    // useRef(initial) — returns a stable handle. .get()/.set() do not
    // flip STATE_DIRTY (writes don't re-render). NR_PTR so the
    // returned slot-handle is NaN-boxed; receiver-method dispatch on
    // the result unboxes back to i64.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useRef",
        class_filter: None,
        runtime: "js_perry_tui_use_ref",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "get",
        class_filter: Some("RefBox"),
        runtime: "js_perry_tui_ref_get",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "set",
        class_filter: Some("RefBox"),
        runtime: "js_perry_tui_ref_set",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    // useApp() — returns the singleton App handle.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useApp",
        class_filter: None,
        runtime: "js_perry_tui_use_app",
        args: &[],
        ret: NR_PTR,
    },
    // app.exit() / app.waitUntilExit() — class_filter routes only when
    // the receiver was registered as a "TuiApp" instance (see
    // destructuring.rs). These match ink's useApp() shape.
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "exit",
        class_filter: Some("TuiApp"),
        runtime: "js_perry_tui_app_exit",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "waitUntilExit",
        class_filter: Some("TuiApp"),
        runtime: "js_perry_tui_app_wait_until_exit",
        args: &[],
        ret: NR_VOID,
    },
    // useStdout() — returns the singleton Stdout handle.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useStdout",
        class_filter: None,
        runtime: "js_perry_tui_use_stdout",
        args: &[],
        ret: NR_PTR,
    },
    // stdout.write(s) / stdout.columns() / stdout.rows().
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "write",
        class_filter: Some("TuiStdout"),
        runtime: "js_perry_tui_stdout_write",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "columns",
        class_filter: Some("TuiStdout"),
        runtime: "js_perry_tui_stdout_columns",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "rows",
        class_filter: Some("TuiStdout"),
        runtime: "js_perry_tui_stdout_rows",
        args: &[],
        ret: NR_F64,
    },
    // Top-level `waitUntilExit()` — receiver-less convenience that
    // blocks until exit().
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "waitUntilExit",
        class_filter: None,
        runtime: "js_perry_tui_wait_until_exit",
        args: &[],
        ret: NR_VOID,
    },
    // ---- perry/tui Phase 3 — focus management (#679) ----
    // useFocus(autoFocus, isActive) — returns 1.0 when this widget is
    // currently focused, else 0.0. Auto-focus on first render when
    // autoFocus=1 and no widget is focused yet.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useFocus",
        class_filter: None,
        runtime: "js_perry_tui_use_focus",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "focusNext",
        class_filter: None,
        runtime: "js_perry_tui_focus_next",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "focusPrevious",
        class_filter: None,
        runtime: "js_perry_tui_focus_previous",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "focus",
        class_filter: None,
        runtime: "js_perry_tui_focus",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useFocusManager",
        class_filter: None,
        runtime: "js_perry_tui_use_focus_manager",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "focusNext",
        class_filter: Some("FocusManager"),
        runtime: "js_perry_tui_focus_manager_focus_next",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "focusPrevious",
        class_filter: Some("FocusManager"),
        runtime: "js_perry_tui_focus_manager_focus_previous",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "focus",
        class_filter: Some("FocusManager"),
        runtime: "js_perry_tui_focus_manager_focus",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    // ========== readline (#347 Phase 1) ==========
    // createInterface(opts) returns a Handle (i64, NaN-boxed POINTER).
    // Instance methods take that Handle as the first arg via has_receiver.
    // Callbacks come in as NA_PTR (unboxed *const ClosureHeader as i64).
    NativeModSig {
        module: "readline",
        has_receiver: false,
        method: "createInterface",
        class_filter: None,
        runtime: "js_readline_create_interface",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "readline",
        has_receiver: true,
        method: "question",
        class_filter: None,
        runtime: "js_readline_question",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "readline",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_readline_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "readline",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_readline_close",
        args: &[],
        ret: NR_VOID,
    },
];
