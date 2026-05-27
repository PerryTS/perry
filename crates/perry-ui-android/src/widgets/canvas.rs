//! Canvas — ImageView with Bitmap-backed Canvas drawing

use crate::jni_bridge;
use jni::objects::{GlobalRef, JObject, JValue};
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
}

struct CanvasState {
    width: i32,
    height: i32,
    density: f32,
    cmds: Vec<DrawCmd>,
    last_cmds: Vec<DrawCmd>,
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

const JS_TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

extern "C" {
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(obj: *mut c_void, key: *const c_void, value: f64);
    fn js_object_get_field_by_name_f64(obj: *mut c_void, key: *const c_void) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut c_void;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_promise_resolved(value: f64) -> *mut c_void;
    fn js_promise_rejected(reason: f64) -> *mut c_void;
}

fn js_key(name: &[u8]) -> *mut c_void {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as u32) }
}

fn set_image_field(obj: *mut c_void, name: &[u8], value: f64) {
    unsafe { js_object_set_field_by_name(obj, js_key(name), value) }
}

fn resolved_image_promise(handle: i64, width: f64, height: f64) -> i64 {
    unsafe {
        let obj = js_object_alloc(0, 4);
        if obj.is_null() {
            let msg = b"Failed to allocate Canvas image object";
            let reason =
                js_nanbox_string(js_string_from_bytes(msg.as_ptr(), msg.len() as u32) as i64);
            return js_promise_rejected(reason) as i64;
        }
        set_image_field(obj, b"__perryImageHandle", handle as f64);
        set_image_field(obj, b"width", width);
        set_image_field(obj, b"height", height);
        set_image_field(obj, b"ready", f64::from_bits(JS_TAG_TRUE));
        js_promise_resolved(js_nanbox_pointer(obj as i64)) as i64
    }
}

fn rejected_image_promise(message: &str) -> i64 {
    unsafe {
        let reason =
            js_nanbox_string(js_string_from_bytes(message.as_ptr(), message.len() as u32) as i64);
        js_promise_rejected(reason) as i64
    }
}

fn image_handle_from_arg(image: i64) -> i64 {
    if image <= 0 {
        return image;
    }
    let key = js_key(b"__perryImageHandle");
    let value = unsafe { js_object_get_field_by_name_f64(image as *mut c_void, key) };
    if value.is_finite() && value > 0.0 {
        value as i64
    } else {
        image
    }
}

fn command_batch_renders(commands: &[DrawCmd]) -> bool {
    commands.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::Clear | DrawCmd::Stroke(..) | DrawCmd::FillGradient(..)
        )
    })
}

pub fn create(width: f64, height: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);

    // Get display density to convert dp to px
    let resources = env
        .call_method(
            &activity,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )
        .expect("getResources")
        .l()
        .expect("resources");
    let display_metrics = env
        .call_method(
            &resources,
            "getDisplayMetrics",
            "()Landroid/util/DisplayMetrics;",
            &[],
        )
        .expect("getDisplayMetrics")
        .l()
        .expect("displayMetrics");
    let density = env
        .get_field(&display_metrics, "density", "F")
        .expect("density")
        .f()
        .expect("float");

    let w = (width as f32 * density) as i32;
    let h = (height as f32 * density) as i32;

    // Create ImageView
    let image_view = env
        .new_object(
            "android/widget/ImageView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create ImageView");

    // Set explicit layout params so the ImageView has a visible size
    let layout_params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(w), JValue::Int(h)],
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &image_view,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&layout_params)],
    );

    // Scale type: FIT_XY so the bitmap fills the allocated space
    let scale_class = env
        .find_class("android/widget/ImageView$ScaleType")
        .expect("ScaleType");
    let fit_xy = env
        .get_static_field(
            &scale_class,
            "FIT_XY",
            "Landroid/widget/ImageView$ScaleType;",
        )
        .expect("FIT_XY")
        .l()
        .expect("scale type");
    let _ = env.call_method(
        &image_view,
        "setScaleType",
        "(Landroid/widget/ImageView$ScaleType;)V",
        &[JValue::Object(&fit_xy)],
    );

    // Create initial bitmap and set it
    create_and_set_bitmap(&mut env, &image_view, w, h);

    let global = env
        .new_global_ref(image_view)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);

    CANVAS_STATES.lock().unwrap().insert(
        handle,
        CanvasState {
            width: w,
            height: h,
            density,
            cmds: Vec::new(),
            last_cmds: Vec::new(),
        },
    );

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

fn create_and_set_bitmap(env: &mut jni::JNIEnv, image_view: &JObject, w: i32, h: i32) {
    // Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    let config_class = env
        .find_class("android/graphics/Bitmap$Config")
        .expect("Bitmap$Config");
    let argb_config = env
        .get_static_field(
            &config_class,
            "ARGB_8888",
            "Landroid/graphics/Bitmap$Config;",
        )
        .expect("ARGB_8888")
        .l()
        .expect("config object");

    let bitmap = env
        .call_static_method(
            "android/graphics/Bitmap",
            "createBitmap",
            "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
            &[JValue::Int(w), JValue::Int(h), JValue::Object(&argb_config)],
        )
        .expect("createBitmap")
        .l()
        .expect("bitmap");

    let _ = env.call_method(
        image_view,
        "setImageBitmap",
        "(Landroid/graphics/Bitmap;)V",
        &[JValue::Object(&bitmap)],
    );
}

fn repaint(handle: i64) {
    let cmds = {
        let mut states = CANVAS_STATES.lock().unwrap();
        states.get_mut(&handle).map(|st| {
            let cmds = if st.cmds.is_empty() || !command_batch_renders(&st.cmds) {
                st.last_cmds.clone()
            } else {
                let cmds = std::mem::take(&mut st.cmds);
                st.last_cmds = cmds.clone();
                cmds
            };
            (st.width, st.height, cmds)
        })
    };

    if let Some((w, h, cmds)) = cmds {
        if let Some(view_ref) = super::get_widget(handle) {
            let mut env = jni_bridge::get_env();
            let _ = env.push_local_frame(64);

            // Create fresh bitmap
            let config_class = env
                .find_class("android/graphics/Bitmap$Config")
                .expect("Bitmap$Config");
            let argb_config = env
                .get_static_field(
                    &config_class,
                    "ARGB_8888",
                    "Landroid/graphics/Bitmap$Config;",
                )
                .expect("ARGB_8888")
                .l()
                .expect("config object");

            let bitmap = env
                .call_static_method(
                    "android/graphics/Bitmap",
                    "createBitmap",
                    "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
                    &[JValue::Int(w), JValue::Int(h), JValue::Object(&argb_config)],
                )
                .expect("createBitmap")
                .l()
                .expect("bitmap");

            // Create Canvas from bitmap
            let canvas = env
                .new_object(
                    "android/graphics/Canvas",
                    "(Landroid/graphics/Bitmap;)V",
                    &[JValue::Object(&bitmap)],
                )
                .expect("Failed to create Canvas");

            // Create Paint
            let paint = env
                .new_object("android/graphics/Paint", "()V", &[])
                .expect("Failed to create Paint");

            // Anti-alias
            let _ = env.call_method(&paint, "setAntiAlias", "(Z)V", &[JValue::Bool(1)]);

            // Replay commands
            let mut path_points: Vec<(f32, f32)> = Vec::new();

            for cmd in &cmds {
                match cmd {
                    DrawCmd::Clear => {
                        // Fill with transparent (clear the bitmap)
                        // Use PorterDuff.Mode.CLEAR to erase all pixels
                        let mode_class = env
                            .find_class("android/graphics/PorterDuff$Mode")
                            .expect("PorterDuff$Mode");
                        let clear_mode = env
                            .get_static_field(
                                &mode_class,
                                "CLEAR",
                                "Landroid/graphics/PorterDuff$Mode;",
                            )
                            .expect("CLEAR")
                            .l()
                            .expect("mode");
                        let _ = env.call_method(
                            &canvas,
                            "drawColor",
                            "(ILandroid/graphics/PorterDuff$Mode;)V",
                            &[JValue::Int(0), JValue::Object(&clear_mode)],
                        );
                    }
                    DrawCmd::BeginPath => {
                        path_points.clear();
                    }
                    DrawCmd::MoveTo(x, y) => {
                        path_points.push((*x, *y));
                    }
                    DrawCmd::LineTo(x, y) => {
                        path_points.push((*x, *y));
                    }
                    DrawCmd::Stroke(color, line_width) => {
                        let _ = env.call_method(&paint, "setColor", "(I)V", &[JValue::Int(*color)]);
                        let _ = env.call_method(
                            &paint,
                            "setStrokeWidth",
                            "(F)V",
                            &[JValue::Float(*line_width)],
                        );
                        // Paint.Style.STROKE = 1
                        let style_class = env
                            .find_class("android/graphics/Paint$Style")
                            .expect("Paint$Style");
                        let stroke_style = env
                            .get_static_field(
                                &style_class,
                                "STROKE",
                                "Landroid/graphics/Paint$Style;",
                            )
                            .expect("STROKE")
                            .l()
                            .expect("style");
                        let _ = env.call_method(
                            &paint,
                            "setStyle",
                            "(Landroid/graphics/Paint$Style;)V",
                            &[JValue::Object(&stroke_style)],
                        );

                        for i in 1..path_points.len() {
                            let (x1, y1) = path_points[i - 1];
                            let (x2, y2) = path_points[i];
                            let _ = env.call_method(
                                &canvas,
                                "drawLine",
                                "(FFFFLandroid/graphics/Paint;)V",
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
                        if path_points.len() >= 3 {
                            // Build Android Path from accumulated path_points
                            let path = env
                                .new_object("android/graphics/Path", "()V", &[])
                                .expect("Failed to create Path");
                            let (sx, sy) = path_points[0];
                            let _ = env.call_method(
                                &path,
                                "moveTo",
                                "(FF)V",
                                &[JValue::Float(sx), JValue::Float(sy)],
                            );
                            for i in 1..path_points.len() {
                                let (px, py) = path_points[i];
                                let _ = env.call_method(
                                    &path,
                                    "lineTo",
                                    "(FF)V",
                                    &[JValue::Float(px), JValue::Float(py)],
                                );
                            }
                            let _ = env.call_method(&path, "close", "()V", &[]);

                            // Create LinearGradient shader
                            let (x1, y1, x2, y2) = if *direction < 0.5 {
                                (0.0f32, 0.0f32, 0.0f32, h as f32) // vertical
                            } else {
                                (0.0f32, 0.0f32, w as f32, 0.0f32) // horizontal
                            };

                            let tile_class = env
                                .find_class("android/graphics/Shader$TileMode")
                                .expect("TileMode");
                            let clamp = env
                                .get_static_field(
                                    &tile_class,
                                    "CLAMP",
                                    "Landroid/graphics/Shader$TileMode;",
                                )
                                .expect("CLAMP")
                                .l()
                                .expect("clamp");

                            let gradient = env
                                .new_object(
                                    "android/graphics/LinearGradient",
                                    "(FFFFIILandroid/graphics/Shader$TileMode;)V",
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
                                "setShader",
                                "(Landroid/graphics/Shader;)Landroid/graphics/Shader;",
                                &[JValue::Object(&gradient)],
                            );

                            // Set FILL style
                            let style_class = env
                                .find_class("android/graphics/Paint$Style")
                                .expect("Paint$Style");
                            let fill_style = env
                                .get_static_field(
                                    &style_class,
                                    "FILL",
                                    "Landroid/graphics/Paint$Style;",
                                )
                                .expect("FILL")
                                .l()
                                .expect("style");
                            let _ = env.call_method(
                                &paint,
                                "setStyle",
                                "(Landroid/graphics/Paint$Style;)V",
                                &[JValue::Object(&fill_style)],
                            );

                            let _ = env.call_method(
                                &canvas,
                                "drawPath",
                                "(Landroid/graphics/Path;Landroid/graphics/Paint;)V",
                                &[JValue::Object(&path), JValue::Object(&paint)],
                            );

                            // Clear shader
                            let _ = env.call_method(
                                &paint,
                                "setShader",
                                "(Landroid/graphics/Shader;)Landroid/graphics/Shader;",
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
                            .call_method(bitmap_obj, "getWidth", "()I", &[])
                            .ok()
                            .and_then(|v| v.i().ok())
                            .unwrap_or(0) as f32;
                        let bitmap_h = env
                            .call_method(bitmap_obj, "getHeight", "()I", &[])
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
                                "android/graphics/Rect",
                                "(IIII)V",
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
                                "android/graphics/RectF",
                                "(FFFF)V",
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
                            "drawBitmap",
                            "(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V",
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
                "setImageBitmap",
                "(Landroid/graphics/Bitmap;)V",
                &[JValue::Object(&bitmap)],
            );

            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
        }
    }
}

pub fn clear(handle: i64) {
    {
        let mut states = CANVAS_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&handle) {
            state.cmds.clear();
            state.last_cmds.clear();
            state.cmds.push(DrawCmd::Clear);
        }
    }
    repaint(handle);
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
    repaint(handle);
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
    repaint(handle);
}

fn argb(a: f64, r: f64, g: f64, b: f64) -> i32 {
    let ai = (a * 255.0) as u32;
    let ri = (r * 255.0) as u32;
    let gi = (g * 255.0) as u32;
    let bi = (b * 255.0) as u32;
    ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32
}

pub fn load_image(path_ptr: *const u8) -> i64 {
    let path = crate::app::str_from_header(path_ptr).to_string();
    if let Some(handle) = IMAGE_CACHE.lock().unwrap().get(&path).copied() {
        if let Some((width, height)) = CANVAS_IMAGE_SIZES.lock().unwrap().get(&handle).copied() {
            return resolved_image_promise(handle, width, height);
        }
        return rejected_image_promise("Cached Canvas image handle was missing");
    }
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    let mut bitmap = JObject::null();
    if !path.starts_with('/') {
        if let Ok(asset_mgr) = env.call_method(
            &activity,
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            &[],
        ) {
            if let Ok(mgr) = asset_mgr.l() {
                let jpath = env.new_string(&path).expect("asset path string");
                if let Ok(stream_val) = env.call_method(
                    &mgr,
                    "open",
                    "(Ljava/lang/String;)Ljava/io/InputStream;",
                    &[JValue::Object(&jpath)],
                ) {
                    if let Ok(stream_obj) = stream_val.l() {
                        if !stream_obj.is_null() {
                            if let Ok(bmp_val) = env.call_static_method(
                                "android/graphics/BitmapFactory",
                                "decodeStream",
                                "(Ljava/io/InputStream;)Landroid/graphics/Bitmap;",
                                &[JValue::Object(&stream_obj)],
                            ) {
                                bitmap = bmp_val.l().unwrap_or_else(|_| JObject::null());
                            }
                            let _ = env.call_method(&stream_obj, "close", "()V", &[]);
                        }
                    }
                }
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_clear();
                }
            }
        }
    }

    if bitmap.is_null() {
        let jpath = env.new_string(&path).expect("bitmap path string");
        if let Ok(bmp_val) = env.call_static_method(
            "android/graphics/BitmapFactory",
            "decodeFile",
            "(Ljava/lang/String;)Landroid/graphics/Bitmap;",
            &[JValue::Object(&jpath)],
        ) {
            bitmap = bmp_val.l().unwrap_or_else(|_| JObject::null());
        }
    }

    let result = if bitmap.is_null() {
        rejected_image_promise(&format!("Failed to load image: {path}"))
    } else {
        let width = env
            .call_method(&bitmap, "getWidth", "()I", &[])
            .ok()
            .and_then(|v| v.i().ok())
            .unwrap_or(0) as f64;
        let height = env
            .call_method(&bitmap, "getHeight", "()I", &[])
            .ok()
            .and_then(|v| v.i().ok())
            .unwrap_or(0) as f64;
        let global = env
            .new_global_ref(bitmap)
            .expect("Failed to create image global ref");
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
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    result
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
    repaint(handle);
}
