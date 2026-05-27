//! `PERRY_UI_INSTANCE_TABLE` — receiver-based perry/ui method calls.

use super::*;

pub const PERRY_UI_INSTANCE_TABLE: &[MethodRow] = &[
    // ---- Window instance methods ----
    MethodRow {
        method: "show",
        runtime: "perry_ui_window_show",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "hide",
        runtime: "perry_ui_window_hide",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "close",
        runtime: "perry_ui_window_close",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setBody",
        runtime: "perry_ui_window_set_body",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setSize",
        runtime: "perry_ui_window_set_size",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "onFocusLost",
        runtime: "perry_ui_window_on_focus_lost",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- State instance methods ----
    MethodRow {
        method: "value",
        runtime: "perry_ui_state_get",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "set",
        runtime: "perry_ui_state_set",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Canvas instance methods ----
    MethodRow {
        method: "setFillColor",
        runtime: "perry_ui_canvas_set_fill_color",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setStrokeColor",
        runtime: "perry_ui_canvas_set_stroke_color",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setLineWidth",
        runtime: "perry_ui_canvas_set_line_width",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "fillRect",
        runtime: "perry_ui_canvas_fill_rect",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "strokeRect",
        runtime: "perry_ui_canvas_stroke_rect",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "clearRect",
        runtime: "perry_ui_canvas_clear_rect",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "beginPath",
        runtime: "perry_ui_canvas_begin_path",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "moveTo",
        runtime: "perry_ui_canvas_move_to",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "lineTo",
        runtime: "perry_ui_canvas_line_to",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "arc",
        runtime: "perry_ui_canvas_arc",
        args: &[
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "closePath",
        runtime: "perry_ui_canvas_close_path",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "fill",
        runtime: "perry_ui_canvas_fill",
        args: &[],
        ret: ReturnKind::Void,
    },
    // `stroke()` maps to perry_ui_canvas_stroke_path (no-arg stateful form).
    // The older perry_ui_canvas_stroke(h,r,g,b,a,lw) stateless form is kept
    // for the legacy fill_gradient API and is not removed.
    MethodRow {
        method: "stroke",
        runtime: "perry_ui_canvas_stroke_path",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "fillText",
        runtime: "perry_ui_canvas_fill_text",
        args: &[ArgKind::Str, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setFont",
        runtime: "perry_ui_canvas_set_font",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // drawImage(image, dx, dy) / drawImage(image, dx, dy, dw, dh) /
    // drawImage(image, sx, sy, sw, sh, dx, dy, dw, dh) are normalized by
    // native lowering into this 9-argument runtime shape. Negative widths
    // ask the runtime to use the image's intrinsic dimensions.
    MethodRow {
        method: "drawImage",
        runtime: "perry_ui_canvas_draw_image",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
];
