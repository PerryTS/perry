//! Canvas — ImageView with Bitmap-backed Canvas drawing

use crate::jni_bridge;
use crate::jni_bridge::GlobalRef;
use jni::objects::JObject;
use jni::JValue;
use perry_ui::canvas_font::{parse_canvas_font, CanvasFont};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

/// Drawing commands accumulated and replayed.
#[derive(Clone, Debug)]
pub enum DrawCmd {
    BeginPath,
    MoveTo(f32, f32),
    LineTo(f32, f32),
    Stroke(i32, f32),            // ARGB color, line_width
    FillGradient(i32, i32, f64), // color1_argb, color2_argb, direction
    DrawImage {
        image: i64,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
    },
    Clear,
    ClearRect(f32, f32, f32, f32),
    // Stateful (HTML-canvas-style) API. Colors are ARGB; coordinates and
    // widths are already scaled to device pixels.
    SetFillColor(i32),
    SetStrokeColor(i32),
    SetLineWidth(f32),
    FillRect(f32, f32, f32, f32),
    StrokeRect(f32, f32, f32, f32),
    ClosePath,
    Arc(f32, f32, f32, f32, f32), // cx, cy, r, start_rad, end_rad
    FillPath,
    StrokePath,
    /// Sizes stay in CSS pixels; replay scales them by the density.
    SetFont(CanvasFont),
    FillText(String, f32, f32),
}

/// Path segment recorded during replay for the stateful fill()/stroke().
#[derive(Clone, Copy, Debug)]
enum PathSeg {
    Move(f32, f32),
    Line(f32, f32),
    Arc(f32, f32, f32, f32, f32),
    Close,
}

struct CanvasState {
    width: i32,
    height: i32,
    density: f32,
    cmds: Vec<DrawCmd>,
    last_cmds: Vec<DrawCmd>,
    /// Drawing state in effect after the last taken batch; see `carry_state`.
    carried: Vec<DrawCmd>,
}

// Global (not thread-local) because canvas is created on the perry-native thread
// but drawing commands arrive from the UI thread via setInterval callbacks.
static CANVAS_STATES: LazyLock<Mutex<HashMap<i64, CanvasState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CANVAS_IMAGES: LazyLock<Mutex<HashMap<i64, GlobalRef>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static IMAGE_CACHE: LazyLock<Mutex<HashMap<String, i64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CANVAS_IMAGE_SIZES: LazyLock<Mutex<HashMap<i64, (f64, f64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_IMAGE_HANDLE: AtomicI64 = AtomicI64::new(1);

extern "C" {
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut c_void;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_promise_rejected(reason: f64) -> *mut c_void;
}

fn resolved_image_promise(handle: i64, width: f64, height: f64) -> i64 {
    perry_ffi::runtime_layouts::image_result(handle, width, height)
}

fn rejected_image_promise(message: &str) -> i64 {
    unsafe {
        let reason =
            js_nanbox_string(js_string_from_bytes(message.as_ptr(), message.len() as u32) as i64);
        js_promise_rejected(reason) as i64
    }
}

fn command_batch_renders(commands: &[DrawCmd]) -> bool {
    commands.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::Clear
                | DrawCmd::ClearRect(..)
                | DrawCmd::Stroke(..)
                | DrawCmd::FillGradient(..)
                | DrawCmd::FillRect(..)
                | DrawCmd::StrokeRect(..)
                | DrawCmd::FillPath
                | DrawCmd::StrokePath
                | DrawCmd::DrawImage { .. }
                | DrawCmd::FillText(..)
        )
    })
}

/// The points of an HTML canvas `arc` (clockwise, a span of 2π or more the whole circle), as
/// chords of about two pixels, at least 8 per full turn, for the point list the colored
/// stroke() and fillGradient read.
fn arc_chords(x: f32, y: f32, r: f32, start: f32, end: f32) -> impl Iterator<Item = (f32, f32)> {
    let tau = std::f32::consts::TAU;
    let sweep = if end - start >= tau {
        tau
    } else {
        (end - start).rem_euclid(tau)
    };
    let min_steps = (8.0 * sweep / tau).ceil().max(1.0) as usize;
    let steps = ((sweep * r / 2.0).ceil() as usize).clamp(min_steps, 4096);
    (0..=steps).map(move |i| {
        let angle = start + sweep * i as f32 / steps as f32;
        (x + r * angle.cos(), y + r * angle.sin())
    })
}

/// Prefixes `commands` with the drawing state earlier batches left set, so a saved batch
/// replays alone and the state lasts as in HTML canvas: a setter until the next one, the
/// current path until the next BeginPath. Records the state `commands` leave into `state`.
fn carry_state(state: &mut Vec<DrawCmd>, commands: Vec<DrawCmd>) -> Vec<DrawCmd> {
    let mut batch = state.clone();
    let is_path = |cmd: &DrawCmd| {
        matches!(
            cmd,
            DrawCmd::MoveTo(..) | DrawCmd::LineTo(..) | DrawCmd::Arc(..) | DrawCmd::ClosePath
        )
    };
    for cmd in &commands {
        if matches!(cmd, DrawCmd::BeginPath) {
            state.retain(|kept| !is_path(kept));
        } else if is_path(cmd) {
            state.push(cmd.clone());
        } else if matches!(
            cmd,
            DrawCmd::SetFillColor(_)
                | DrawCmd::SetStrokeColor(_)
                | DrawCmd::SetLineWidth(_)
                | DrawCmd::SetFont(_)
        ) {
            state.retain(|kept| std::mem::discriminant(kept) != std::mem::discriminant(cmd));
            state.push(cmd.clone());
        }
    }
    batch.extend(commands);
    batch
}

pub fn create(width: f64, height: f64) -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);

        // Get display density to convert dp to px
        let resources = env
            .call_method(
                &activity,
                jni::jni_str!("getResources"),
                jni::jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )
            .expect("getResources")
            .l()
            .expect("resources");
        let display_metrics = env
            .call_method(
                &resources,
                jni::jni_str!("getDisplayMetrics"),
                jni::jni_sig!("()Landroid/util/DisplayMetrics;"),
                &[],
            )
            .expect("getDisplayMetrics")
            .l()
            .expect("displayMetrics");
        let density = env
            .get_field(
                &display_metrics,
                jni::jni_str!("density"),
                jni::jni_sig!("F"),
            )
            .expect("density")
            .f()
            .expect("float");

        let w = (width as f32 * density) as i32;
        let h = (height as f32 * density) as i32;

        // Create ImageView
        let image_view = env
            .new_object(
                jni::jni_str!("android/widget/ImageView"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create ImageView");

        // Set explicit layout params so the ImageView has a visible size
        let layout_params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(w), JValue::Int(h)],
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &image_view,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&layout_params)],
        );

        // Scale type: FIT_XY so the bitmap fills the allocated space
        let scale_class = env
            .find_class(jni::jni_str!("android/widget/ImageView$ScaleType"))
            .expect("ScaleType");
        let fit_xy = env
            .get_static_field(
                &scale_class,
                jni::jni_str!("FIT_XY"),
                jni::jni_sig!("Landroid/widget/ImageView$ScaleType;"),
            )
            .expect("FIT_XY")
            .l()
            .expect("scale type");
        let _ = env.call_method(
            &image_view,
            jni::jni_str!("setScaleType"),
            jni::jni_sig!("(Landroid/widget/ImageView$ScaleType;)V"),
            &[JValue::Object(&fit_xy)],
        );

        // Create initial bitmap and set it
        create_and_set_bitmap(env, &image_view, w, h);

        let global =
            jni_bridge::new_global_ref(env, image_view).expect("Failed to create global ref");
        let handle = super::register_widget(global);

        CANVAS_STATES.lock().unwrap().insert(
            handle,
            CanvasState {
                width: w,
                height: h,
                density,
                cmds: Vec::new(),
                last_cmds: Vec::new(),
                carried: Vec::new(),
            },
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}

fn create_and_set_bitmap(env: &mut jni::Env, image_view: &JObject, w: i32, h: i32) {
    // Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    let config_class = env
        .find_class(jni::jni_str!("android/graphics/Bitmap$Config"))
        .expect("Bitmap$Config");
    let argb_config = env
        .get_static_field(
            &config_class,
            jni::jni_str!("ARGB_8888"),
            jni::jni_sig!("Landroid/graphics/Bitmap$Config;"),
        )
        .expect("ARGB_8888")
        .l()
        .expect("config object");

    let bitmap = env
        .call_static_method(
            jni::jni_str!("android/graphics/Bitmap"),
            jni::jni_str!("createBitmap"),
            jni::jni_sig!("(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;"),
            &[JValue::Int(w), JValue::Int(h), JValue::Object(&argb_config)],
        )
        .expect("createBitmap")
        .l()
        .expect("bitmap");

    let _ = env.call_method(
        image_view,
        jni::jni_str!("setImageBitmap"),
        jni::jni_sig!("(Landroid/graphics/Bitmap;)V"),
        &[JValue::Object(&bitmap)],
    );
}

/// Canvases with commands queued since the last flush. Drawing calls only
/// mark a canvas dirty; `flush_dirty()` (called once per UI pump tick, after
/// JS timers ran) renders each dirty canvas once. This makes every call
/// between two ticks one frame — the same model as iOS/macOS, where
/// `drawRect:` drains the pending batch. Previously each `stroke()` /
/// `fillGradient()` repainted immediately into a fresh bitmap, so only the
/// last draw call of a frame survived (e.g. a filled area followed by its
/// outline showed only the outline).
static DIRTY: LazyLock<Mutex<Vec<i64>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn mark_dirty(handle: i64) {
    let mut dirty = DIRTY.lock().unwrap();
    if !dirty.contains(&handle) {
        dirty.push(handle);
    }
}

/// Render every canvas that received commands since the last call.
pub fn flush_dirty() {
    let handles = std::mem::take(&mut *DIRTY.lock().unwrap());
    for handle in handles {
        repaint(handle);
    }
}

fn paint_set_style(env: &mut jni::Env, paint: &JObject, fill: bool) {
    let Ok(style_class) = env.find_class(jni::jni_str!("android/graphics/Paint$Style")) else {
        return;
    };
    let style = if fill {
        env.get_static_field(
            &style_class,
            jni::jni_str!("FILL"),
            jni::jni_sig!("Landroid/graphics/Paint$Style;"),
        )
    } else {
        env.get_static_field(
            &style_class,
            jni::jni_str!("STROKE"),
            jni::jni_sig!("Landroid/graphics/Paint$Style;"),
        )
    };
    if let Ok(style) = style.and_then(|v| v.l()) {
        let _ = env.call_method(
            paint,
            jni::jni_str!("setStyle"),
            jni::jni_sig!("(Landroid/graphics/Paint$Style;)V"),
            &[JValue::Object(&style)],
        );
    }
}

fn paint_set_color_width(env: &mut jni::Env, paint: &JObject, color: i32, width: f32) {
    let _ = env.call_method(
        paint,
        jni::jni_str!("setColor"),
        jni::jni_sig!("(I)V"),
        &[JValue::Int(color)],
    );
    let _ = env.call_method(
        paint,
        jni::jni_str!("setStrokeWidth"),
        jni::jni_sig!("(F)V"),
        &[JValue::Float(width)],
    );
}

/// Build an `android.graphics.Path` from recorded segments.
fn build_path<'local>(env: &mut jni::Env<'local>, segs: &[PathSeg]) -> Option<JObject<'local>> {
    let path = env
        .new_object(
            jni::jni_str!("android/graphics/Path"),
            jni::jni_sig!("()V"),
            &[],
        )
        .ok()?;
    for seg in segs {
        match *seg {
            PathSeg::Move(x, y) => {
                let _ = env.call_method(
                    &path,
                    jni::jni_str!("moveTo"),
                    jni::jni_sig!("(FF)V"),
                    &[JValue::Float(x), JValue::Float(y)],
                );
            }
            PathSeg::Line(x, y) => {
                let _ = env.call_method(
                    &path,
                    jni::jni_str!("lineTo"),
                    jni::jni_sig!("(FF)V"),
                    &[JValue::Float(x), JValue::Float(y)],
                );
            }
            PathSeg::Close => {
                let _ = env.call_method(&path, jni::jni_str!("close"), jni::jni_sig!("()V"), &[]);
            }
            PathSeg::Arc(cx, cy, r, start, end) => {
                // HTML's clockwise sweep, the rule arc_chords uses; a positive sweep runs clockwise on screen.
                let tau = std::f32::consts::TAU;
                let sweep = if end - start >= tau {
                    tau
                } else {
                    (end - start).rem_euclid(tau)
                }
                .to_degrees();
                // Canvas arcs connect from the current point, as arcTo with forceMoveTo=false
                // does. arcTo wraps a 360° sweep to nothing, so a full turn is two halves.
                let parts = if sweep >= 359.99 { 2 } else { 1 };
                for part in 0..parts {
                    let part_sweep = sweep / parts as f32;
                    let _ = env.call_method(
                        &path,
                        jni::jni_str!("arcTo"),
                        jni::jni_sig!("(FFFFFFZ)V"),
                        &[
                            JValue::Float(cx - r),
                            JValue::Float(cy - r),
                            JValue::Float(cx + r),
                            JValue::Float(cy + r),
                            JValue::Float(start.to_degrees() + part_sweep * part as f32),
                            JValue::Float(part_sweep),
                            JValue::Bool(false),
                        ],
                    );
                }
            }
        }
    }
    let _ = env.exception_clear();
    Some(path)
}

fn create_typeface<'local>(
    env: &mut jni::Env<'local>,
    family: &JObject,
    style: i32,
) -> Option<JObject<'local>> {
    env.call_static_method(
        jni::jni_str!("android/graphics/Typeface"),
        jni::jni_str!("create"),
        jni::jni_sig!("(Ljava/lang/String;I)Landroid/graphics/Typeface;"),
        &[JValue::Object(family), JValue::Int(style)],
    )
    .ok()?
    .l()
    .ok()
}

/// The first family Android has, generics included, else the default typeface.
fn typeface<'local>(env: &mut jni::Env<'local>, font: &CanvasFont) -> Option<JObject<'local>> {
    // Typeface.NORMAL = 0, BOLD = 1, ITALIC = 2, BOLD_ITALIC = 3.
    let style = font.bold as i32 | (font.italic as i32) << 1;
    // An unknown family resolves to the same cached object as the default typeface.
    let fallback = create_typeface(env, &JObject::null(), style)?;
    for family in &font.families {
        let lower = family.to_ascii_lowercase();
        let (name, generic) = match lower.as_str() {
            "sans-serif" | "system-ui" => ("sans-serif", true),
            "serif" | "monospace" => (lower.as_str(), true),
            _ => (family.as_str(), false),
        };
        let jname = env.new_string(name).ok()?;
        let typeface = create_typeface(env, &jname, style)?;
        if generic || !env.is_same_object(&typeface, &fallback).unwrap_or(true) {
            return Some(typeface);
        }
    }
    Some(fallback)
}

fn repaint(handle: i64) {
    let cmds = {
        let mut states = CANVAS_STATES.lock().unwrap();
        states.get_mut(&handle).map(|st| {
            let cmds = if st.cmds.is_empty() || !command_batch_renders(&st.cmds) {
                st.last_cmds.clone()
            } else {
                let cmds = carry_state(&mut st.carried, std::mem::take(&mut st.cmds));
                st.last_cmds = cmds.clone();
                cmds
            };
            (st.width, st.height, st.density, cmds)
        })
    };

    if let Some((w, h, density, cmds)) = cmds {
        if let Some(view_ref) = super::get_widget(handle) {
            jni_bridge::with_env(|env| {
                let _ = jni_bridge::push_local_frame(env, 64);

                // Create fresh bitmap
                let config_class = env
                    .find_class(jni::jni_str!("android/graphics/Bitmap$Config"))
                    .expect("Bitmap$Config");
                let argb_config = env
                    .get_static_field(
                        &config_class,
                        jni::jni_str!("ARGB_8888"),
                        jni::jni_sig!("Landroid/graphics/Bitmap$Config;"),
                    )
                    .expect("ARGB_8888")
                    .l()
                    .expect("config object");

                let bitmap = env
                    .call_static_method(
                        jni::jni_str!("android/graphics/Bitmap"),
                        jni::jni_str!("createBitmap"),
                        jni::jni_sig!(
                            "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;"
                        ),
                        &[JValue::Int(w), JValue::Int(h), JValue::Object(&argb_config)],
                    )
                    .expect("createBitmap")
                    .l()
                    .expect("bitmap");

                // Create Canvas from bitmap
                let canvas = env
                    .new_object(
                        jni::jni_str!("android/graphics/Canvas"),
                        jni::jni_sig!("(Landroid/graphics/Bitmap;)V"),
                        &[JValue::Object(&bitmap)],
                    )
                    .expect("Failed to create Canvas");

                // Create Paint
                let paint = env
                    .new_object(
                        jni::jni_str!("android/graphics/Paint"),
                        jni::jni_sig!("()V"),
                        &[],
                    )
                    .expect("Failed to create Paint");

                // Anti-alias
                let _ = env.call_method(
                    &paint,
                    jni::jni_str!("setAntiAlias"),
                    jni::jni_sig!("(Z)V"),
                    &[JValue::Bool(true)],
                );

                // Replay commands
                let mut path_points: Vec<(f32, f32)> = Vec::new();
                let mut segs: Vec<PathSeg> = Vec::new();
                // Stateful API current values (canvas defaults: black, 1px).
                let mut cur_fill: i32 = 0xFF00_0000u32 as i32;
                let mut cur_stroke: i32 = 0xFF00_0000u32 as i32;
                let mut cur_width: f32 = 1.0;
                let mut cur_font = CanvasFont::default();

                for cmd in &cmds {
                    match cmd {
                        DrawCmd::Clear | DrawCmd::ClearRect(..) => {
                            // PorterDuff.Mode.CLEAR erases every pixel inside the clip.
                            if let DrawCmd::ClearRect(x, y, w, h) = cmd {
                                // Density scaling can push the rect past the f32 range, and
                                // Skia ignores a non-finite clip, which would clear everything.
                                if ![*x, *y, *x + *w, *y + *h].iter().all(|v| v.is_finite()) {
                                    continue;
                                }
                                let _ = env.call_method(
                                    &canvas,
                                    jni::jni_str!("save"),
                                    jni::jni_sig!("()I"),
                                    &[],
                                );
                                let _ = env.call_method(
                                    &canvas,
                                    jni::jni_str!("clipRect"),
                                    jni::jni_sig!("(FFFF)Z"),
                                    &[
                                        JValue::Float(*x),
                                        JValue::Float(*y),
                                        JValue::Float(*x + *w),
                                        JValue::Float(*y + *h),
                                    ],
                                );
                            }
                            let mode_class = env
                                .find_class(jni::jni_str!("android/graphics/PorterDuff$Mode"))
                                .expect("PorterDuff$Mode");
                            let clear_mode = env
                                .get_static_field(
                                    &mode_class,
                                    jni::jni_str!("CLEAR"),
                                    jni::jni_sig!("Landroid/graphics/PorterDuff$Mode;"),
                                )
                                .expect("CLEAR")
                                .l()
                                .expect("mode");
                            let _ = env.call_method(
                                &canvas,
                                jni::jni_str!("drawColor"),
                                jni::jni_sig!("(ILandroid/graphics/PorterDuff$Mode;)V"),
                                &[JValue::Int(0), JValue::Object(&clear_mode)],
                            );
                            if matches!(cmd, DrawCmd::ClearRect(..)) {
                                let _ = env.call_method(
                                    &canvas,
                                    jni::jni_str!("restore"),
                                    jni::jni_sig!("()V"),
                                    &[],
                                );
                            }
                        }
                        DrawCmd::BeginPath => {
                            path_points.clear();
                            segs.clear();
                        }
                        DrawCmd::MoveTo(x, y) => {
                            path_points.push((*x, *y));
                            segs.push(PathSeg::Move(*x, *y));
                        }
                        DrawCmd::LineTo(x, y) => {
                            path_points.push((*x, *y));
                            segs.push(PathSeg::Line(*x, *y));
                        }
                        DrawCmd::SetFillColor(c) => cur_fill = *c,
                        DrawCmd::SetFont(font) => cur_font = font.clone(),
                        DrawCmd::FillText(text, x, y) => {
                            let _ = jni_bridge::push_local_frame(env, 16);
                            paint_set_color_width(env, &paint, cur_fill, cur_width);
                            paint_set_style(env, &paint, true);
                            let _ = env.call_method(
                                &paint,
                                jni::jni_str!("setTextSize"),
                                jni::jni_sig!("(F)V"),
                                &[JValue::Float(cur_font.size as f32 * density)],
                            );
                            if let Some(typeface) = typeface(env, &cur_font) {
                                let _ = env.call_method(
                                    &paint,
                                    jni::jni_str!("setTypeface"),
                                    jni::jni_sig!(
                                        "(Landroid/graphics/Typeface;)Landroid/graphics/Typeface;"
                                    ),
                                    &[JValue::Object(&typeface)],
                                );
                            }
                            if let Ok(jtext) = env.new_string(text) {
                                let _ = env.call_method(
                                    &canvas,
                                    jni::jni_str!("drawText"),
                                    jni::jni_sig!(
                                        "(Ljava/lang/String;FFLandroid/graphics/Paint;)V"
                                    ),
                                    &[
                                        JValue::Object(&jtext),
                                        JValue::Float(*x),
                                        JValue::Float(*y),
                                        JValue::Object(&paint),
                                    ],
                                );
                            }
                            unsafe {
                                let _ = jni_bridge::pop_local_frame(env, &JObject::null());
                            }
                        }
                        DrawCmd::SetStrokeColor(c) => cur_stroke = *c,
                        DrawCmd::SetLineWidth(w) => cur_width = *w,
                        DrawCmd::ClosePath => segs.push(PathSeg::Close),
                        DrawCmd::Arc(cx, cy, r, sa, ea) => {
                            path_points.extend(arc_chords(*cx, *cy, *r, *sa, *ea));
                            segs.push(PathSeg::Arc(*cx, *cy, *r, *sa, *ea));
                        }
                        DrawCmd::FillRect(x, y, w, h) | DrawCmd::StrokeRect(x, y, w, h) => {
                            let fill = matches!(cmd, DrawCmd::FillRect(..));
                            let color = if fill { cur_fill } else { cur_stroke };
                            paint_set_color_width(env, &paint, color, cur_width);
                            paint_set_style(env, &paint, fill);
                            let _ = env.call_method(
                                &canvas,
                                jni::jni_str!("drawRect"),
                                jni::jni_sig!("(FFFFLandroid/graphics/Paint;)V"),
                                &[
                                    JValue::Float(*x),
                                    JValue::Float(*y),
                                    JValue::Float(*x + *w),
                                    JValue::Float(*y + *h),
                                    JValue::Object(&paint),
                                ],
                            );
                        }
                        DrawCmd::FillPath | DrawCmd::StrokePath => {
                            let fill = matches!(cmd, DrawCmd::FillPath);
                            if segs.is_empty() {
                                continue;
                            }
                            if let Some(path) = build_path(env, &segs) {
                                let color = if fill { cur_fill } else { cur_stroke };
                                paint_set_color_width(env, &paint, color, cur_width);
                                paint_set_style(env, &paint, fill);
                                let _ = env.call_method(
                                    &canvas,
                                    jni::jni_str!("drawPath"),
                                    jni::jni_sig!(
                                        "(Landroid/graphics/Path;Landroid/graphics/Paint;)V"
                                    ),
                                    &[JValue::Object(&path), JValue::Object(&paint)],
                                );
                            }
                        }
                        DrawCmd::Stroke(color, line_width) => {
                            let _ = env.call_method(
                                &paint,
                                jni::jni_str!("setColor"),
                                jni::jni_sig!("(I)V"),
                                &[JValue::Int(*color)],
                            );
                            let _ = env.call_method(
                                &paint,
                                jni::jni_str!("setStrokeWidth"),
                                jni::jni_sig!("(F)V"),
                                &[JValue::Float(*line_width)],
                            );
                            // Paint.Style.STROKE = 1
                            let style_class = env
                                .find_class(jni::jni_str!("android/graphics/Paint$Style"))
                                .expect("Paint$Style");
                            let stroke_style = env
                                .get_static_field(
                                    &style_class,
                                    jni::jni_str!("STROKE"),
                                    jni::jni_sig!("Landroid/graphics/Paint$Style;"),
                                )
                                .expect("STROKE")
                                .l()
                                .expect("style");
                            let _ = env.call_method(
                                &paint,
                                jni::jni_str!("setStyle"),
                                jni::jni_sig!("(Landroid/graphics/Paint$Style;)V"),
                                &[JValue::Object(&stroke_style)],
                            );

                            for i in 1..path_points.len() {
                                let (x1, y1) = path_points[i - 1];
                                let (x2, y2) = path_points[i];
                                let _ = env.call_method(
                                    &canvas,
                                    jni::jni_str!("drawLine"),
                                    jni::jni_sig!("(FFFFLandroid/graphics/Paint;)V"),
                                    &[
                                        JValue::Float(x1),
                                        JValue::Float(y1),
                                        JValue::Float(x2),
                                        JValue::Float(y2),
                                        JValue::Object(&paint),
                                    ],
                                );
                            }
                        }
                        DrawCmd::FillGradient(color1, color2, direction) => {
                            // Fills the area between the path and the bottom edge, as an area chart.
                            if path_points.len() >= 2 {
                                let path = env
                                    .new_object(
                                        jni::jni_str!("android/graphics/Path"),
                                        jni::jni_sig!("()V"),
                                        &[],
                                    )
                                    .expect("Failed to create Path");
                                let (sx, sy) = path_points[0];
                                let _ = env.call_method(
                                    &path,
                                    jni::jni_str!("moveTo"),
                                    jni::jni_sig!("(FF)V"),
                                    &[JValue::Float(sx), JValue::Float(sy)],
                                );
                                for i in 1..path_points.len() {
                                    let (px, py) = path_points[i];
                                    let _ = env.call_method(
                                        &path,
                                        jni::jni_str!("lineTo"),
                                        jni::jni_sig!("(FF)V"),
                                        &[JValue::Float(px), JValue::Float(py)],
                                    );
                                }
                                let last_x = path_points[path_points.len() - 1].0;
                                for x in [last_x, sx] {
                                    let _ = env.call_method(
                                        &path,
                                        jni::jni_str!("lineTo"),
                                        jni::jni_sig!("(FF)V"),
                                        &[JValue::Float(x), JValue::Float(h as f32)],
                                    );
                                }
                                let _ = env.call_method(
                                    &path,
                                    jni::jni_str!("close"),
                                    jni::jni_sig!("()V"),
                                    &[],
                                );

                                // Create LinearGradient shader
                                let (x1, y1, x2, y2) = if *direction < 0.5 {
                                    (0.0f32, 0.0f32, 0.0f32, h as f32) // vertical
                                } else {
                                    (0.0f32, 0.0f32, w as f32, 0.0f32) // horizontal
                                };

                                let tile_class = env
                                    .find_class(jni::jni_str!("android/graphics/Shader$TileMode"))
                                    .expect("TileMode");
                                let clamp = env
                                    .get_static_field(
                                        &tile_class,
                                        jni::jni_str!("CLAMP"),
                                        jni::jni_sig!("Landroid/graphics/Shader$TileMode;"),
                                    )
                                    .expect("CLAMP")
                                    .l()
                                    .expect("clamp");

                                let gradient = env
                                    .new_object(
                                        jni::jni_str!("android/graphics/LinearGradient"),
                                        jni::jni_sig!(
                                            "(FFFFIILandroid/graphics/Shader$TileMode;)V"
                                        ),
                                        &[
                                            JValue::Float(x1),
                                            JValue::Float(y1),
                                            JValue::Float(x2),
                                            JValue::Float(y2),
                                            JValue::Int(*color1),
                                            JValue::Int(*color2),
                                            JValue::Object(&clamp),
                                        ],
                                    )
                                    .expect("LinearGradient");

                                let _ = env.call_method(
                                    &paint,
                                    jni::jni_str!("setShader"),
                                    jni::jni_sig!(
                                        "(Landroid/graphics/Shader;)Landroid/graphics/Shader;"
                                    ),
                                    &[JValue::Object(&gradient)],
                                );

                                // Set FILL style
                                let style_class = env
                                    .find_class(jni::jni_str!("android/graphics/Paint$Style"))
                                    .expect("Paint$Style");
                                let fill_style = env
                                    .get_static_field(
                                        &style_class,
                                        jni::jni_str!("FILL"),
                                        jni::jni_sig!("Landroid/graphics/Paint$Style;"),
                                    )
                                    .expect("FILL")
                                    .l()
                                    .expect("style");
                                let _ = env.call_method(
                                    &paint,
                                    jni::jni_str!("setStyle"),
                                    jni::jni_sig!("(Landroid/graphics/Paint$Style;)V"),
                                    &[JValue::Object(&fill_style)],
                                );

                                let _ = env.call_method(
                                    &canvas,
                                    jni::jni_str!("drawPath"),
                                    jni::jni_sig!(
                                        "(Landroid/graphics/Path;Landroid/graphics/Paint;)V"
                                    ),
                                    &[JValue::Object(&path), JValue::Object(&paint)],
                                );

                                // Clear shader
                                let _ = env.call_method(
                                    &paint,
                                    jni::jni_str!("setShader"),
                                    jni::jni_sig!(
                                        "(Landroid/graphics/Shader;)Landroid/graphics/Shader;"
                                    ),
                                    &[JValue::Object(&jni::objects::JObject::null())],
                                );
                            }
                        }
                        DrawCmd::DrawImage {
                            image,
                            sx,
                            sy,
                            sw,
                            sh,
                            dx,
                            dy,
                            dw,
                            dh,
                        } => {
                            let images = CANVAS_IMAGES.lock().unwrap();
                            let Some(bitmap_ref) = images.get(image) else {
                                continue;
                            };
                            let bitmap_obj = bitmap_ref.as_obj();
                            let bitmap_w = env
                                .call_method(
                                    bitmap_obj,
                                    jni::jni_str!("getWidth"),
                                    jni::jni_sig!("()I"),
                                    &[],
                                )
                                .ok()
                                .and_then(|v| v.i().ok())
                                .unwrap_or(0) as f32;
                            let bitmap_h = env
                                .call_method(
                                    bitmap_obj,
                                    jni::jni_str!("getHeight"),
                                    jni::jni_sig!("()I"),
                                    &[],
                                )
                                .ok()
                                .and_then(|v| v.i().ok())
                                .unwrap_or(0) as f32;
                            let src_w = if *sw > 0.0 { *sw } else { bitmap_w };
                            let src_h = if *sh > 0.0 { *sh } else { bitmap_h };
                            let dst_w = if *dw > 0.0 { *dw } else { src_w };
                            let dst_h = if *dh > 0.0 { *dh } else { src_h };
                            if src_w <= 0.0 || src_h <= 0.0 || dst_w <= 0.0 || dst_h <= 0.0 {
                                continue;
                            }
                            let src_rect = env
                                .new_object(
                                    jni::jni_str!("android/graphics/Rect"),
                                    jni::jni_sig!("(IIII)V"),
                                    &[
                                        JValue::Int(*sx as i32),
                                        JValue::Int(*sy as i32),
                                        JValue::Int((*sx + src_w) as i32),
                                        JValue::Int((*sy + src_h) as i32),
                                    ],
                                )
                                .expect("Rect");
                            let dst_rect = env
                                .new_object(
                                    jni::jni_str!("android/graphics/RectF"),
                                    jni::jni_sig!("(FFFF)V"),
                                    &[
                                        JValue::Float(*dx),
                                        JValue::Float(*dy),
                                        JValue::Float(*dx + dst_w),
                                        JValue::Float(*dy + dst_h),
                                    ],
                                )
                                .expect("RectF");
                            let _ = env.call_method(
                            &canvas,
                            jni::jni_str!("drawBitmap"),
                            jni::jni_sig!("(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V"),
                            &[
                                JValue::Object(bitmap_obj),
                                JValue::Object(&src_rect),
                                JValue::Object(&dst_rect),
                                JValue::Object(&paint),
                            ],
                        );
                        }
                    }
                }

                // Set bitmap on ImageView
                let _ = env.call_method(
                    view_ref.as_obj(),
                    jni::jni_str!("setImageBitmap"),
                    jni::jni_sig!("(Landroid/graphics/Bitmap;)V"),
                    &[JValue::Object(&bitmap)],
                );

                unsafe {
                    let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
                }
            })
        }
    }
}

pub fn clear(handle: i64) {
    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            carry_state(&mut state.carried, std::mem::take(&mut state.cmds));
            state.last_cmds.clear();
            state.cmds.push(DrawCmd::Clear);
        }
    }
    mark_dirty(handle);
}

pub fn begin_path(handle: i64) {
    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            state.cmds.push(DrawCmd::BeginPath);
        }
    }
}

pub fn move_to(handle: i64, x: f64, y: f64) {
    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            let d = state.density;
            state.cmds.push(DrawCmd::MoveTo(x as f32 * d, y as f32 * d));
        }
    }
}

pub fn line_to(handle: i64, x: f64, y: f64) {
    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            let d = state.density;
            state.cmds.push(DrawCmd::LineTo(x as f32 * d, y as f32 * d));
        }
    }
}

pub fn stroke(handle: i64, r: f64, g: f64, b: f64, a: f64, line_width: f64) {
    let ai = (a * 255.0) as u32;
    let ri = (r * 255.0) as u32;
    let gi = (g * 255.0) as u32;
    let bi = (b * 255.0) as u32;
    let color = ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32;

    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            let d = state.density;
            state
                .cmds
                .push(DrawCmd::Stroke(color, line_width as f32 * d));
        }
    }
    mark_dirty(handle);
}

pub fn fill_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    let c1 = argb(a1, r1, g1, b1);
    let c2 = argb(a2, r2, g2, b2);

    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            state.cmds.push(DrawCmd::FillGradient(c1, c2, direction));
        }
    }
    mark_dirty(handle);
}

fn argb(a: f64, r: f64, g: f64, b: f64) -> i32 {
    let ai = (a * 255.0) as u32;
    let ri = (r * 255.0) as u32;
    let gi = (g * 255.0) as u32;
    let bi = (b * 255.0) as u32;
    ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32
}

pub fn load_image(path_ptr: *const u8) -> i64 {
    let path = unsafe { crate::app::str_from_header(path_ptr) }.to_string();
    if let Some(handle) = IMAGE_CACHE.lock().unwrap().get(&path).copied() {
        if let Some((width, height)) = CANVAS_IMAGE_SIZES.lock().unwrap().get(&handle).copied() {
            return resolved_image_promise(handle, width, height);
        }
        return rejected_image_promise("Cached Canvas image handle was missing");
    }
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let mut bitmap = JObject::null();
        if !path.starts_with('/') {
            if let Ok(asset_mgr) = env.call_method(
                &activity,
                jni::jni_str!("getAssets"),
                jni::jni_sig!("()Landroid/content/res/AssetManager;"),
                &[],
            ) {
                if let Ok(mgr) = asset_mgr.l() {
                    let jpath = env.new_string(&path).expect("asset path string");
                    if let Ok(stream_val) = env.call_method(
                        &mgr,
                        jni::jni_str!("open"),
                        jni::jni_sig!("(Ljava/lang/String;)Ljava/io/InputStream;"),
                        &[JValue::Object(&jpath)],
                    ) {
                        if let Ok(stream_obj) = stream_val.l() {
                            if !stream_obj.is_null() {
                                if let Ok(bmp_val) = env.call_static_method(
                                    jni::jni_str!("android/graphics/BitmapFactory"),
                                    jni::jni_str!("decodeStream"),
                                    jni::jni_sig!(
                                        "(Ljava/io/InputStream;)Landroid/graphics/Bitmap;"
                                    ),
                                    &[JValue::Object(&stream_obj)],
                                ) {
                                    bitmap = bmp_val.l().unwrap_or_else(|_| JObject::null());
                                }
                                let _ = env.call_method(
                                    &stream_obj,
                                    jni::jni_str!("close"),
                                    jni::jni_sig!("()V"),
                                    &[],
                                );
                            }
                        }
                    }
                    if env.exception_check() {
                        let _ = env.exception_clear();
                    }
                }
            }
        }

        if bitmap.is_null() {
            let jpath = env.new_string(&path).expect("bitmap path string");
            if let Ok(bmp_val) = env.call_static_method(
                jni::jni_str!("android/graphics/BitmapFactory"),
                jni::jni_str!("decodeFile"),
                jni::jni_sig!("(Ljava/lang/String;)Landroid/graphics/Bitmap;"),
                &[JValue::Object(&jpath)],
            ) {
                bitmap = bmp_val.l().unwrap_or_else(|_| JObject::null());
            }
        }

        let result = if bitmap.is_null() {
            rejected_image_promise(&format!("Failed to load image: {path}"))
        } else {
            let width = env
                .call_method(
                    &bitmap,
                    jni::jni_str!("getWidth"),
                    jni::jni_sig!("()I"),
                    &[],
                )
                .ok()
                .and_then(|v| v.i().ok())
                .unwrap_or(0) as f64;
            let height = env
                .call_method(
                    &bitmap,
                    jni::jni_str!("getHeight"),
                    jni::jni_sig!("()I"),
                    &[],
                )
                .ok()
                .and_then(|v| v.i().ok())
                .unwrap_or(0) as f64;
            let global =
                jni_bridge::new_global_ref(env, bitmap).expect("Failed to create image global ref");
            let handle = NEXT_IMAGE_HANDLE.fetch_add(1, Ordering::Relaxed);
            CANVAS_IMAGES.lock().unwrap().insert(handle, global);
            CANVAS_IMAGE_SIZES
                .lock()
                .unwrap()
                .insert(handle, (width, height));
            IMAGE_CACHE.lock().unwrap().insert(path, handle);
            resolved_image_promise(handle, width, height)
        };
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        result
    })
}

pub fn draw_image(
    handle: i64,
    image: i64,
    sx: f64,
    sy: f64,
    sw: f64,
    sh: f64,
    dx: f64,
    dy: f64,
    dw: f64,
    dh: f64,
) {
    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            let d = state.density;
            state.cmds.push(DrawCmd::DrawImage {
                image,
                sx: sx as f32,
                sy: sy as f32,
                sw: sw as f32,
                sh: sh as f32,
                dx: dx as f32 * d,
                dy: dy as f32 * d,
                dw: if dw > 0.0 { dw as f32 * d } else { dw as f32 },
                dh: if dh > 0.0 { dh as f32 * d } else { dh as f32 },
            });
        }
    }
    mark_dirty(handle);
}

// =============================================================================
// Stateful canvas API (setFillColor / fillRect / fill / stroke / arc ...)
// =============================================================================

fn push_cmd(handle: i64, make: impl FnOnce(f32) -> DrawCmd, renders: bool) {
    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            let d = state.density;
            state.cmds.push(make(d));
        } else {
            return;
        }
    }
    if renders {
        mark_dirty(handle);
    }
}

pub fn set_fill_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    push_cmd(handle, |_| DrawCmd::SetFillColor(argb(a, r, g, b)), false);
}

pub fn set_stroke_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    push_cmd(handle, |_| DrawCmd::SetStrokeColor(argb(a, r, g, b)), false);
}

pub fn set_line_width(handle: i64, width: f64) {
    push_cmd(handle, |d| DrawCmd::SetLineWidth(width as f32 * d), false);
}

pub fn fill_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(
        handle,
        |d| DrawCmd::FillRect(x as f32 * d, y as f32 * d, w as f32 * d, h as f32 * d),
        true,
    );
}

/// Clear a rectangle to transparent, keeping what earlier commands drew outside it.
pub fn clear_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    // Normalized so `clipRect` gets ordered endpoints for a negative width or height.
    let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
    let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
    push_cmd(
        handle,
        |d| DrawCmd::ClearRect(x as f32 * d, y as f32 * d, w as f32 * d, h as f32 * d),
        true,
    );
}

pub fn stroke_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(
        handle,
        |d| DrawCmd::StrokeRect(x as f32 * d, y as f32 * d, w as f32 * d, h as f32 * d),
        true,
    );
}

pub fn arc(handle: i64, x: f64, y: f64, r: f64, start: f64, end: f64) {
    if !([x, y, r, start, end].iter().all(|v| v.is_finite()) && r >= 0.0) {
        return;
    }
    // Whole turns come off in f64, so huge angles keep their sweep once cast to f32.
    let turns = (start / std::f64::consts::TAU).floor() * std::f64::consts::TAU;
    let (start, end) = (start - turns, end - turns);
    push_cmd(
        handle,
        |d| {
            DrawCmd::Arc(
                x as f32 * d,
                y as f32 * d,
                r as f32 * d,
                start as f32,
                end as f32,
            )
        },
        false,
    );
}

pub fn close_path(handle: i64) {
    push_cmd(handle, |_| DrawCmd::ClosePath, false);
}

pub fn fill(handle: i64) {
    push_cmd(handle, |_| DrawCmd::FillPath, true);
}

pub fn stroke_path(handle: i64) {
    push_cmd(handle, |_| DrawCmd::StrokePath, true);
}

/// An invalid font spec keeps the previous font, as HTML canvas does.
pub fn set_font(handle: i64, spec_ptr: *const u8) {
    let spec = unsafe { crate::app::str_from_header(spec_ptr) };
    if let Some(font) = parse_canvas_font(&spec) {
        push_cmd(handle, |_| DrawCmd::SetFont(font), false);
    }
}

pub fn fill_text(handle: i64, text_ptr: *const u8, x: f64, y: f64) {
    if !(x.is_finite() && y.is_finite()) {
        return;
    }
    let text = unsafe { crate::app::str_from_header(text_ptr) };
    push_cmd(
        handle,
        |d| DrawCmd::FillText(text, x as f32 * d, y as f32 * d),
        true,
    );
}
