//! Canvas — ImageView with Bitmap-backed Canvas drawing

use crate::jni_bridge;
use jni::objects::{JObject, JValue};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Drawing commands accumulated and replayed.
#[derive(Clone, Debug)]
pub enum DrawCmd {
    BeginPath,
    MoveTo(f32, f32),
    LineTo(f32, f32),
    Stroke(i32, f32),            // ARGB color, line_width
    FillGradient(i32, i32, f64), // color1_argb, color2_argb, direction
    Clear,
}

struct CanvasState {
    width: i32,
    height: i32,
    density: f32,
    cmds: Vec<DrawCmd>,
}

// Global (not thread-local) because canvas is created on the perry-native thread
// but drawing commands arrive from the UI thread via setInterval callbacks.
static CANVAS_STATES: LazyLock<Mutex<HashMap<i64, CanvasState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
        let states = CANVAS_STATES.lock().unwrap();
        states
            .get(&handle)
            .map(|st| (st.width, st.height, st.cmds.clone()))
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
