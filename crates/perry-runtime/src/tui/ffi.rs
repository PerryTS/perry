//! C ABI surface called by perry-codegen.

use std::sync::Mutex;
use std::sync::OnceLock;

use crate::string::StringHeader;

use super::cell::Grid;
use super::color::{parse_color, Color};
use super::render;
use super::style::{Edges, Length};
use super::tree::{box_add_child, register, Node};

/// Singleton grid — sized to the current terminal at first render.
static GRID: OnceLock<Mutex<Grid>> = OnceLock::new();

fn grid() -> &'static Mutex<Grid> {
    GRID.get_or_init(|| {
        let (w, h) = current_term_size();
        Mutex::new(Grid::new(w, h))
    })
}

/// Read the current terminal size via TIOCGWINSZ. Falls back to 80x24
/// when stdout isn't a TTY.
fn current_term_size() -> (u16, u16) {
    #[cfg(unix)]
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            return (ws.ws_col, ws.ws_row);
        }
    }
    (80, 24)
}

// ---------------------------------------------------------------------------
// Widget factories
// ---------------------------------------------------------------------------

/// `Text(content)` — single-line text widget. Returns the raw widget
/// handle as i64; the dispatch table's NR_PTR contract NaN-boxes it.
/// (Returning f64 here works accidentally — Rust compiles
/// `f64::from_bits(u64)` as a register-to-register move so the u64
/// stays in RAX while the f64 ends up in XMM0 with the same bit
/// pattern, and the IR's `call i64` reads RAX. But that's a fragile
/// happenstance; explicit i64 is the canonical contract.)
#[no_mangle]
pub extern "C" fn js_perry_tui_text(content_ptr: *const StringHeader) -> i64 {
    let content = unsafe { read_string(content_ptr) };
    register(Node::Text {
        content,
        fg: Color::Default,
        bg: Color::Default,
        style: super::cell::Style::default(),
    })
}

/// `Text(content, { fg, bg, bold, italic, underline, reverse })` — same as
/// `js_perry_tui_text` but with style props applied. `fg` / `bg` are
/// strings (named palette like `"red"`, hex `#rrggbb`, or empty for
/// default); `style_bits` packs the four boolean style flags into the
/// existing `Style` u8. Used by the codegen when a Text call has a
/// trailing options object literal. (#405 Phase 3.5.)
#[no_mangle]
pub extern "C" fn js_perry_tui_text_styled(
    content_ptr: *const StringHeader,
    fg_ptr: *const StringHeader,
    bg_ptr: *const StringHeader,
    style_bits: f64,
) -> i64 {
    let content = unsafe { read_string(content_ptr) };
    let fg = parse_color(&unsafe { read_string(fg_ptr) });
    let bg = parse_color(&unsafe { read_string(bg_ptr) });
    let bits = style_bits.max(0.0) as u8;
    register(Node::Text {
        content,
        fg,
        bg,
        style: super::cell::Style(bits),
    })
}

/// `Box()` — empty container. Children are added via
/// `js_perry_tui_box_add_child`. Style props (flexDirection, gap, …)
/// are set via the `js_perry_tui_box_set_*` family below — typically
/// emitted by the codegen as a follow-up to a Box-with-style call shape
/// `Box({ flexDirection: "row" }, [children])`.
#[no_mangle]
pub extern "C" fn js_perry_tui_box() -> i64 {
    register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::style::BoxStyle::default(),
    })
}

/// Mutate a Box's style. Wraps `tree::with_node_mut` so the per-FFI
/// boilerplate stays small. Silently no-ops on non-Box handles.
fn with_box_style_mut(handle: i64, f: impl FnOnce(&mut super::style::BoxStyle)) {
    super::tree::with_node_mut(handle, |n| {
        if let Node::Box { style, .. } = n {
            f(style);
        }
    });
}

/// `Box(parent).addChildrenFromArray(arr)` — iterate a runtime JS
/// array of widget handles and add each one as a child of `parent`.
/// Used when `Box(...)` is called with a non-literal children
/// expression (e.g. `messages.map(m => Text(m))`) — the codegen's
/// Box recogniser can't expand the children at compile time, so it
/// emits one call to this helper instead.
///
/// `children_array` is the unboxed `*mut ArrayHeader` pointer (i64).
/// Elements are NaN-boxed POINTER widget handles (as f64); we unbox
/// each to its raw i64 widget handle before calling `box_add_child`.
/// (#679 follow-up: pre-fix Box(map_result) silently produced an
/// empty container.)
#[no_mangle]
pub extern "C" fn js_perry_tui_box_add_children_array(parent: i64, children_array: i64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if children_array == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let len = crate::array::js_array_get_length(children_array);
    for i in 0..len {
        let child_f64 = crate::array::js_array_get_element_f64(children_array, i);
        // Children are NaN-boxed POINTER widget handles. Unbox by
        // stripping the high 16 bits of the NaN-box tag to recover
        // the raw i64 widget handle. (Same pattern run.rs uses to
        // extract a Widget handle from the component's return.)
        let bits = child_f64.to_bits();
        let child_handle = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if child_handle != 0 {
            super::tree::box_add_child(parent, child_handle);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `Box.flexDirection = "row" | "column"` — emitted by the codegen
/// when a Box style object includes `flexDirection`.
#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_flex_direction(
    handle: i64,
    value_ptr: *const StringHeader,
) -> f64 {
    let s = unsafe { read_string(value_ptr) };
    let dir = super::style::parse_flex_direction(&s);
    with_box_style_mut(handle, |style| style.flex_direction = dir);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_justify_content(
    handle: i64,
    value_ptr: *const StringHeader,
) -> f64 {
    let s = unsafe { read_string(value_ptr) };
    let v = super::style::parse_justify_content(&s);
    with_box_style_mut(handle, |style| style.justify_content = v);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_align_items(
    handle: i64,
    value_ptr: *const StringHeader,
) -> f64 {
    let s = unsafe { read_string(value_ptr) };
    let v = super::style::parse_align_items(&s);
    with_box_style_mut(handle, |style| style.align_items = v);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_gap(handle: i64, gap: f64) -> f64 {
    let g = gap.max(0.0) as u16;
    with_box_style_mut(handle, |style| style.gap = g);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_padding(handle: i64, padding: f64) -> f64 {
    let p = padding.max(0.0) as u16;
    with_box_style_mut(handle, |style| style.padding = Edges::all(p));
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Per-side padding setter — emitted by codegen when the user passes
/// `padding: { top, right, bottom, left }`. Missing fields default to
/// 0 cells. (#405 Phase 3.5.)
#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_padding_each(
    handle: i64,
    top: f64,
    right: f64,
    bottom: f64,
    left: f64,
) -> f64 {
    let edges = Edges {
        top: top.max(0.0) as u16,
        right: right.max(0.0) as u16,
        bottom: bottom.max(0.0) as u16,
        left: left.max(0.0) as u16,
    };
    with_box_style_mut(handle, |style| style.padding = edges);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_width(handle: i64, width: f64) -> f64 {
    let w = width.max(0.0) as u16;
    with_box_style_mut(handle, |style| style.width = Some(Length::Cells(w)));
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_height(handle: i64, height: f64) -> f64 {
    let h = height.max(0.0) as u16;
    with_box_style_mut(handle, |style| style.height = Some(Length::Cells(h)));
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Percentage-of-parent width. `pct` is 0.0..=100.0; out-of-range
/// values are clamped. (#405 Phase 3.5.)
#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_width_pct(handle: i64, pct: f64) -> f64 {
    let l = Length::percent(pct as f32);
    with_box_style_mut(handle, |style| style.width = Some(l));
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_height_pct(handle: i64, pct: f64) -> f64 {
    let l = Length::percent(pct as f32);
    with_box_style_mut(handle, |style| style.height = Some(l));
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_flex_grow(handle: i64, grow: f64) -> f64 {
    let g = grow.max(0.0) as u16;
    with_box_style_mut(handle, |style| style.flex_grow = g);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_flex_shrink(handle: i64, shrink: f64) -> f64 {
    let s = shrink.max(0.0) as u16;
    with_box_style_mut(handle, |style| style.flex_shrink = s);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_flex_basis(handle: i64, cells: f64) -> f64 {
    let n = cells.max(0.0) as u16;
    with_box_style_mut(handle, |style| style.flex_basis = Some(Length::Cells(n)));
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn js_perry_tui_box_set_flex_basis_pct(handle: i64, pct: f64) -> f64 {
    let l = Length::percent(pct as f32);
    with_box_style_mut(handle, |style| style.flex_basis = Some(l));
    f64::from_bits(0x7FFC_0000_0000_0001)
}

// ---------------------------------------------------------------------------
// Phase 4 widgets — Spacer + ProgressBar.
// ---------------------------------------------------------------------------

/// `Spacer()` — empty Box with `flex_grow: 1`. In a row layout it
/// pushes siblings apart; in a column layout it pushes them up/down.
/// Equivalent to `Box({ flexGrow: 1 })` — provided as its own FFI for
/// the more discoverable name.
#[no_mangle]
pub extern "C" fn js_perry_tui_spacer() -> i64 {
    let mut s = super::style::BoxStyle::default();
    s.flex_grow = 1;
    super::tree::register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: s,
    })
}

/// `ProgressBar(value, max, width)` — renders `[====    ]`-style filled
/// bar. value/max → fraction of `width` cells filled with `=`; the
/// rest are spaces. Brackets are added at both ends so the widget's
/// total width is `width + 2`. Returns a Text widget handle.
#[no_mangle]
pub extern "C" fn js_perry_tui_progress_bar(value: f64, max: f64, width: f64) -> i64 {
    let w = width.max(1.0) as usize;
    let frac = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (frac * (w as f64)).round() as usize;
    let mut s = String::with_capacity(w + 2);
    s.push('[');
    for _ in 0..filled {
        s.push('=');
    }
    for _ in filled..w {
        s.push(' ');
    }
    s.push(']');
    super::tree::register(Node::Text {
        content: s,
        fg: Color::Default,
        bg: Color::Default,
        style: super::cell::Style::default(),
    })
}

// ---------------------------------------------------------------------------
// Phase 4.5 widgets — Spinner + Input + List + Select + TextArea.
// ---------------------------------------------------------------------------

/// `Spinner(frame)` — animated character cycling through `-\|/` based
/// on a frame counter. Caller bumps the frame number from a state slot
/// to animate; pass 0 for a static dash. Returns a Text widget.
///
/// Returns the raw widget handle as i64 (NOT NaN-boxed). The dispatch
/// table's NR_PTR contract NaN-boxes the result. Returning f64 here
/// would mismatch the IR-declared i64 return type and clobber the
/// value through the System V ABI (i64 in RAX, f64 in XMM0).
#[no_mangle]
pub extern "C" fn js_perry_tui_spinner(frame: f64) -> i64 {
    const CHARS: [char; 4] = ['-', '\\', '|', '/'];
    let idx = (frame.max(0.0) as usize) % CHARS.len();
    let s = CHARS[idx].to_string();
    super::tree::register(Node::Text {
        content: s,
        fg: Color::Default,
        bg: Color::Default,
        style: super::cell::Style::default(),
    })
}

/// `Input(value)` — single-line text input renderer. The widget shows
/// `value` followed by a `_` cursor character. The user wires their
/// own keypress handler (via `useInput`) that mutates a state slot
/// holding the value; the widget is purely visual. Returns a Text
/// widget.
#[no_mangle]
pub extern "C" fn js_perry_tui_input(value_ptr: *const StringHeader) -> i64 {
    let value = unsafe { read_string(value_ptr) };
    let display = format!("{}_", value);
    super::tree::register(Node::Text {
        content: display,
        fg: Color::Default,
        bg: Color::Default,
        style: super::cell::Style::default(),
    })
}

/// `Input(value, cursor)` — single-line text input with the cursor at
/// an arbitrary index inside the value (left/right arrow positioning).
/// Decomposes into a horizontal Box of three Text widgets so the
/// cursor character can be drawn with reverse-video without needing
/// per-cell styled runs in `Node::Text`. (#404.)
///
/// Out-of-range cursor is clamped to `[0, value.chars().count()]`. A
/// cursor at exactly the value's end renders a trailing reverse-video
/// space (matching most terminal text editors' end-of-line cursor).
#[no_mangle]
pub extern "C" fn js_perry_tui_input_at(value_ptr: *const StringHeader, cursor: f64) -> i64 {
    let value = unsafe { read_string(value_ptr) };
    let chars: Vec<char> = value.chars().collect();
    let len = chars.len();
    let c = cursor.clamp(0.0, len as f64) as usize;

    let parent = super::tree::register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::style::BoxStyle {
            flex_direction: super::style::FlexDirection::Row,
            ..Default::default()
        },
    });

    if c > 0 {
        let before: String = chars[..c].iter().collect();
        let w = super::tree::register(Node::Text {
            content: before,
            fg: Color::Default,
            bg: Color::Default,
            style: super::cell::Style::default(),
        });
        super::tree::box_add_child(parent, w);
    }

    let cursor_ch = if c < len {
        chars[c].to_string()
    } else {
        " ".to_string()
    };
    let cursor_widget = super::tree::register(Node::Text {
        content: cursor_ch,
        fg: Color::Default,
        bg: Color::Default,
        style: super::cell::Style(super::cell::Style::REVERSE),
    });
    super::tree::box_add_child(parent, cursor_widget);

    if c < len {
        let after: String = chars[c + 1..].iter().collect();
        if !after.is_empty() {
            let w = super::tree::register(Node::Text {
                content: after,
                fg: Color::Default,
                bg: Color::Default,
                style: super::cell::Style::default(),
            });
            super::tree::box_add_child(parent, w);
        }
    }

    parent
}

/// Read items from a JS array of strings into an owned `Vec<String>`.
/// Used by List / Select. The array is unboxed at the codegen call
/// site (NA_PTR in the dispatch table); each element is read via
/// `js_array_get_f64_unchecked` and converted to a string via the
/// runtime's `js_jsvalue_to_string`.
fn read_string_array(items_ptr: i64) -> Vec<String> {
    use crate::array::{js_array_get_f64_unchecked, js_array_length, ArrayHeader};
    use crate::value::js_jsvalue_to_string;
    let arr = items_ptr as *const ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    unsafe {
        let len = js_array_length(arr);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let v = js_array_get_f64_unchecked(arr, i);
            let s_ptr = js_jsvalue_to_string(v);
            if s_ptr.is_null() {
                out.push(String::new());
                continue;
            }
            let s_len = (*s_ptr).byte_len as usize;
            let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, s_len);
            out.push(String::from_utf8_lossy(bytes).into_owned());
        }
        out
    }
}

/// `List(items, selected)` — vertical list of items as a Box of Text
/// children. The `selected` index (default -1 = no selection) is
/// rendered with reverse-video. Returns a Box handle suitable for
/// adding to a parent layout.
#[no_mangle]
pub extern "C" fn js_perry_tui_list(items_ptr: i64, selected: f64) -> i64 {
    let items = read_string_array(items_ptr);
    let sel = selected as i32;
    let parent = super::tree::register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::style::BoxStyle::default(),
    });
    for (i, item) in items.iter().enumerate() {
        let is_sel = i as i32 == sel;
        let style = if is_sel {
            super::cell::Style(super::cell::Style::REVERSE)
        } else {
            super::cell::Style::default()
        };
        let child = super::tree::register(Node::Text {
            content: item.clone(),
            fg: Color::Default,
            bg: Color::Default,
            style,
        });
        super::tree::box_add_child(parent, child);
    }
    parent
}

/// `Select(items, selected)` — alias for `List` with an enforced
/// non-negative selection. Caller's state holds the selected index;
/// this exists as a separate name for readability and so a future
/// v1.5 can diverge (e.g. add a `>` indicator on the selected row).
#[no_mangle]
pub extern "C" fn js_perry_tui_select(items_ptr: i64, selected: f64) -> i64 {
    js_perry_tui_list(items_ptr, selected.max(0.0))
}

/// `TextArea(value)` — multi-line text renderer. Splits `value` on
/// `\n` and emits one Text per line inside a Box (column layout).
/// Like Input, the widget is purely visual — the user wires keypress
/// → state.set themselves. Returns a Box handle.
#[no_mangle]
pub extern "C" fn js_perry_tui_text_area(value_ptr: *const StringHeader) -> i64 {
    let value = unsafe { read_string(value_ptr) };
    let parent = super::tree::register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::style::BoxStyle::default(),
    });
    for line in value.split('\n') {
        let child = super::tree::register(Node::Text {
            content: line.to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: super::cell::Style::default(),
        });
        super::tree::box_add_child(parent, child);
    }
    parent
}

/// Append a child to a Box. Both args are unboxed POINTER handles.
#[no_mangle]
pub extern "C" fn js_perry_tui_box_add_child(parent: i64, child: i64) -> f64 {
    box_add_child(parent, child);
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
}

// ---------------------------------------------------------------------------
// Phase 4.7 widget — AnimatedSpinner (#403).
// ---------------------------------------------------------------------------

/// Default frame set for `AnimatedSpinner()` with no `frames` opt —
/// the same `-\|/` cycle as the static `Spinner(frame)` v1 widget.
const DEFAULT_SPINNER_FRAMES: &[&str] = &["-", "\\", "|", "/"];

/// Spawn-once timer thread that flips `STATE_DIRTY` at a fixed cadence
/// so an animated spinner re-renders inside the `run()` loop without
/// the user wiring `setInterval` themselves. The 50 ms tick is twice
/// the default 100 ms spinner interval (Nyquist) — fast enough that
/// even a 60 ms spinner re-renders cleanly.
fn ensure_spinner_ticker() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .is_ok()
    {
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            super::state::STATE_DIRTY.store(true, std::sync::atomic::Ordering::Release);
        });
    }
}

/// Process-relative monotonic clock anchored on first call. Used for
/// computing animated-spinner frame indices — `Instant::now()`'s
/// `duration_since(START)` gives us an always-positive elapsed.
fn process_elapsed_ms() -> u128 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let s = START.get_or_init(std::time::Instant::now);
    s.elapsed().as_millis()
}

/// `AnimatedSpinner({ interval, frames })` — render a single Text
/// widget showing the current animation frame. Defaults: 100 ms /
/// frame, `-\|/` cycle. Spawning the global ticker thread (once)
/// guarantees the `run()` loop sees `STATE_DIRTY` and re-renders;
/// for one-shot `render()` outside `run()`, only the snapshot prints
/// (no animation — matches `Spinner(0)` static behavior). (#403.)
#[no_mangle]
pub extern "C" fn js_perry_tui_animated_spinner(interval_ms: f64, frames_ptr: i64) -> i64 {
    ensure_spinner_ticker();
    let interval = if interval_ms > 0.0 {
        interval_ms as u128
    } else {
        100
    };
    let frames_owned: Vec<String>;
    let frames: Vec<&str> = if frames_ptr != 0 {
        frames_owned = read_string_array(frames_ptr);
        if frames_owned.is_empty() {
            DEFAULT_SPINNER_FRAMES.iter().copied().collect()
        } else {
            frames_owned.iter().map(|s| s.as_str()).collect()
        }
    } else {
        DEFAULT_SPINNER_FRAMES.iter().copied().collect()
    };
    let idx = ((process_elapsed_ms() / interval) as usize) % frames.len();
    super::tree::register(Node::Text {
        content: frames[idx].to_string(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::cell::Style::default(),
    })
}

// ---------------------------------------------------------------------------
// Phase 4.6 widgets — Table + Tabs (#402).
// ---------------------------------------------------------------------------

/// Read a 2D JS array of strings (`string[][]`) into a `Vec<Vec<String>>`.
/// Each outer element must itself be an array; non-array elements are
/// treated as a one-cell row containing the stringified value.
fn read_string_2d_array(rows_ptr: i64) -> Vec<Vec<String>> {
    use crate::array::{js_array_get_f64_unchecked, js_array_length, ArrayHeader};
    use crate::value::{js_jsvalue_to_string, JSValue};
    let arr = rows_ptr as *const ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    unsafe {
        let len = js_array_length(arr);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let v = js_array_get_f64_unchecked(arr, i);
            // Detect array via NaN-box pointer tag + GcHeader type byte.
            let bits = v.to_bits();
            let pointer_tag = bits >> 48;
            let inner_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
            if pointer_tag >= 0x7FFD && inner_ptr != 0 {
                // Probably an Array — try via js_array_length first (it
                // returns 0 for non-arrays so a real empty row reads
                // safely as zero cells, matching the spec).
                let row = inner_ptr as *const ArrayHeader;
                let row_len = js_array_length(row);
                let mut row_strs = Vec::with_capacity(row_len as usize);
                for j in 0..row_len {
                    let cell = js_array_get_f64_unchecked(row, j);
                    let s_ptr = js_jsvalue_to_string(cell);
                    row_strs.push(read_string(s_ptr));
                }
                out.push(row_strs);
            } else {
                // Scalar element — promote to a 1-cell row.
                let s_ptr = js_jsvalue_to_string(v);
                out.push(vec![read_string(s_ptr)]);
            }
        }
        let _ = JSValue::undefined();
        out
    }
}

/// Read a JS array of widget handles into a `Vec<i64>`. Used by Tabs
/// to splice per-tab body widgets into the container.
fn read_handle_array(handles_ptr: i64) -> Vec<i64> {
    use crate::array::{js_array_get_f64_unchecked, js_array_length, ArrayHeader};
    let arr = handles_ptr as *const ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    let len = js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let v = js_array_get_f64_unchecked(arr, i);
        // Widget handles are NaN-boxed POINTER values — extract the
        // low 48 bits as a raw handle.
        let h = (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
        out.push(h);
    }
    out
}

/// Pad `s` to `width` chars with trailing spaces. Truncates if longer.
fn pad_right(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        let mut t = String::with_capacity(width);
        for c in s.chars().take(width) {
            t.push(c);
        }
        t
    } else {
        let mut t = String::with_capacity(width);
        t.push_str(s);
        for _ in 0..(width - n) {
            t.push(' ');
        }
        t
    }
}

/// `Table({ headers, rows, selected })` — render a 2D grid as a
/// column-stacked Box of single-row Text widgets. Each row is built
/// by joining padded cells with two-space gaps. The selected row's
/// Text widget is rendered with Style::REVERSE. Returns a Box handle.
/// (#402.)
#[no_mangle]
pub extern "C" fn js_perry_tui_table(headers_ptr: i64, rows_ptr: i64, selected: f64) -> i64 {
    let headers = read_string_array(headers_ptr);
    let rows = read_string_2d_array(rows_ptr);
    let sel = selected as i32;

    // Compute column widths — max of header length and any row's cell
    // length. The grid is sparse-tolerant: rows shorter than `headers`
    // are padded with empty cells; longer rows are clipped.
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in &rows {
        for (i, cell) in row.iter().take(cols).enumerate() {
            let w = cell.chars().count();
            if w > widths[i] {
                widths[i] = w;
            }
        }
    }

    let parent = super::tree::register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::style::BoxStyle::default(),
    });

    // Header row — bold + 2-space cell separator.
    let mut header_line = String::new();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            header_line.push_str("  ");
        }
        header_line.push_str(&pad_right(h, widths[i]));
    }
    let header_widget = super::tree::register(Node::Text {
        content: header_line,
        fg: Color::Default,
        bg: Color::Default,
        style: super::cell::Style(super::cell::Style::BOLD),
    });
    super::tree::box_add_child(parent, header_widget);

    // Data rows — selected row gets reverse-video.
    for (ri, row) in rows.iter().enumerate() {
        let mut line = String::new();
        for ci in 0..cols {
            if ci > 0 {
                line.push_str("  ");
            }
            let cell = row.get(ci).map(|s| s.as_str()).unwrap_or("");
            line.push_str(&pad_right(cell, widths[ci]));
        }
        let style = if ri as i32 == sel {
            super::cell::Style(super::cell::Style::REVERSE)
        } else {
            super::cell::Style::default()
        };
        let row_widget = super::tree::register(Node::Text {
            content: line,
            fg: Color::Default,
            bg: Color::Default,
            style,
        });
        super::tree::box_add_child(parent, row_widget);
    }

    parent
}

/// `Tabs({ tabs, active, body })` — render a horizontal tab bar
/// (active tab in reverse video) followed by the active tab's body
/// widget. `tabs` is the label array, `active` is the 0-based index,
/// `body` is the parallel array of widget handles (one per tab) — only
/// the active body is rendered. Returns a Box handle. (#402.)
#[no_mangle]
pub extern "C" fn js_perry_tui_tabs(tabs_ptr: i64, active: f64, body_ptr: i64) -> i64 {
    let tabs = read_string_array(tabs_ptr);
    let bodies = read_handle_array(body_ptr);
    let active_idx = active.max(0.0) as usize;

    let outer = super::tree::register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::style::BoxStyle {
            flex_direction: super::style::FlexDirection::Column,
            ..Default::default()
        },
    });

    // Tab bar — horizontal Box with one Text per tab + one-space gap.
    let bar = super::tree::register(Node::Box {
        children: Vec::new(),
        fg: Color::Default,
        bg: Color::Default,
        style: super::style::BoxStyle {
            flex_direction: super::style::FlexDirection::Row,
            gap: 1,
            ..Default::default()
        },
    });
    for (i, label) in tabs.iter().enumerate() {
        let style = if i == active_idx {
            super::cell::Style(super::cell::Style::REVERSE)
        } else {
            super::cell::Style::default()
        };
        let tab_widget = super::tree::register(Node::Text {
            content: label.clone(),
            fg: Color::Default,
            bg: Color::Default,
            style,
        });
        super::tree::box_add_child(bar, tab_widget);
    }
    super::tree::box_add_child(outer, bar);

    // Body — only the active tab's widget is mounted. Out-of-range
    // active index just shows the bar with no body (matches React's
    // null-render fallback for missing keys).
    if let Some(body) = bodies.get(active_idx) {
        super::tree::box_add_child(outer, *body);
    }

    outer
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// `render(root)` — paint one frame. Phase 3 (#358) routes through
/// the Taffy layout pass before paint so flexbox styles take effect.
#[no_mangle]
pub extern "C" fn js_perry_tui_render(root: i64) -> f64 {
    let (w, h) = current_term_size();
    let mut g = grid().lock().unwrap();
    g.resize(w, h);
    g.clear_back();
    let rects = super::layout::compute_layout(root, w, h);
    super::tree::paint_with_layout(&mut g, root, &rects);
    render::flush(&mut g);
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Same as `js_perry_tui_render` but exposed to other tui submodules
/// (the render loop in run.rs) without the FFI wrapper.
pub(super) fn paint_root_for_run(root: i64) {
    let (w, h) = current_term_size();
    let mut g = grid().lock().unwrap();
    g.resize(w, h);
    g.clear_back();
    let rects = super::layout::compute_layout(root, w, h);
    super::tree::paint_with_layout(&mut g, root, &rects);
    render::flush(&mut g);
}

/// Initialize the renderer — clear screen and home the cursor.
#[no_mangle]
pub extern "C" fn js_perry_tui_enter() -> f64 {
    render::enter();
    f64::from_bits(0x7FFC_0000_0000_0001)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

unsafe fn read_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let slice = std::slice::from_raw_parts(data, len);
    String::from_utf8_lossy(slice).into_owned()
}
