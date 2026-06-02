use gstreamer_app::prelude::*;
use gtk4::gdk;
use gtk4::gdk_pixbuf;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;

thread_local! {
    static CAMERA_VIEWS: Mutex<HashMap<i64, CameraViewData>> = Mutex::new(HashMap::new());
    static GST_INITIALIZED: RefCell<bool> = RefCell::new(false);
}

extern "C" {
    fn js_closure_call3(closure: *const u8, arg1: f64, arg2: f64, arg3: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

fn ensure_gst_init() -> Result<(), String> {
    GST_INITIALIZED.with(|i| {
        if !*i.borrow() {
            gstreamer::init().map_err(|e| format!("Failed to initialize GStreamer: {:?}", e))?;
            *i.borrow_mut() = true;
        }
        Ok(())
    })
}

struct CameraViewData {
    widget: gtk4::Widget,
    image: gtk4::Image,
    pipeline: Option<gstreamer::Pipeline>,
    is_running: AtomicBool,
    is_frozen: AtomicBool,
    last_frame: RefCell<Option<(Vec<u8>, usize, usize)>>,
    receiver: Option<mpsc::Receiver<FrameData>>,
    frame_callback: Mutex<Option<f64>>,
    frame_count: std::sync::atomic::AtomicU64,
}

struct FrameData {
    data: Vec<u8>,
    width: i32,
    height: i32,
    format: String,
    stride: i32,
}

pub fn create() -> i64 {
    crate::app::ensure_gtk_init();

    let image = gtk4::Image::new();
    image.set_hexpand(true);
    image.set_vexpand(true);

    let data = CameraViewData {
        widget: gtk4::prelude::Cast::upcast(image.clone()),
        image,
        pipeline: None,
        is_running: AtomicBool::new(false),
        is_frozen: AtomicBool::new(false),
        last_frame: RefCell::new(None),
        receiver: None,
        frame_callback: Mutex::new(None),
        frame_count: std::sync::atomic::AtomicU64::new(0),
    };

    let handle = crate::widgets::register_widget(data.widget.clone());
    CAMERA_VIEWS.with(|c| c.lock().unwrap().insert(handle, data));

    handle
}

pub fn start(handle: i64) {
    CAMERA_VIEWS.with(|c| {
        let mut views = c.lock().unwrap();

        let Some(view) = views.get_mut(&handle) else {
            return;
        };

        if view.is_running.load(Ordering::Relaxed) {
            return;
        }

        view.is_running.store(true, Ordering::Relaxed);

        let (sender, receiver) = mpsc::channel();
        view.receiver = Some(receiver);

        let pipeline = match create_pipeline(handle, sender) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[CameraView] Failed to create pipeline: {}", e);
                view.is_running.store(false, Ordering::Relaxed);
                return;
            }
        };

        view.pipeline = Some(pipeline);
        schedule_frame_processing(handle);
    });
}

fn schedule_frame_processing(handle: i64) {
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        let should_continue = std::panic::catch_unwind(|| {
            // Frame bytes are *owned* here (an explicit copy), not a borrowed
            // pointer into `last_frame`. The JS callback is deferred to the GTK
            // idle handler (below), which runs at a lower priority than this
            // 16ms timeout — so a borrowed pointer into `last_frame` could dangle
            // if the next tick overwrites `last_frame` before the idle fires.
            let mut pending_callback: Option<(f64, Vec<u8>, usize, usize)> = None;
            let is_running = CAMERA_VIEWS.with(|c| {
                let mut views = c.lock().unwrap();
                let Some(view) = views.get_mut(&handle) else {
                    return false;
                };

                if !view.is_running.load(Ordering::Relaxed) {
                    return false;
                }

                let receiver = match view.receiver.as_mut() {
                    Some(r) => r,
                    None => return true,
                };

                while let Ok(frame) = receiver.try_recv() {
                    let running = view.is_running.load(Ordering::Relaxed);
                    let frozen = view.is_frozen.load(Ordering::Relaxed);

                    if !running || frozen {
                        continue;
                    }

                    if frame.data.is_empty() {
                        continue;
                    }

                    let (display_data, stride) = match frame.format.as_str() {
                        "YUY2" => {
                            let rgb_data = yuy2_to_rgb(
                                &frame.data,
                                frame.width as usize,
                                frame.height as usize,
                            );
                            (rgb_data, frame.width as usize * 3)
                        }
                        "BGR" => {
                            let rgb_data = bgr_to_rgb(&frame.data);
                            (rgb_data, frame.width as usize * 3)
                        }
                        "BGRA" => {
                            let rgb_data = bgra_to_rgb(
                                &frame.data,
                                frame.width as usize,
                                frame.height as usize,
                                "BGRA",
                            );
                            (rgb_data, frame.width as usize * 3)
                        }
                        "ABGR" => {
                            let rgb_data = bgra_to_rgb(
                                &frame.data,
                                frame.width as usize,
                                frame.height as usize,
                                "ABGR",
                            );
                            (rgb_data, frame.width as usize * 3)
                        }
                        "RGB" => (frame.data.clone(), frame.width as usize * 3),
                        "RGB16" => {
                            let rgb_data = rgb16_to_rgb(&frame.data);
                            (rgb_data, frame.width as usize * 3)
                        }
                        "NV12" => {
                            let rgb_data = nv12_to_rgb(
                                &frame.data,
                                frame.width as usize,
                                frame.height as usize,
                            );
                            (rgb_data, frame.width as usize * 3)
                        }
                        "I420" => {
                            let rgb_data = i420_to_rgb(
                                &frame.data,
                                frame.width as usize,
                                frame.height as usize,
                            );
                            (rgb_data, frame.width as usize * 3)
                        }
                        f => {
                            eprintln!("[CameraView] Unsupported format: {}", f);
                            continue;
                        }
                    };

                    let bytes = gtk4::glib::Bytes::from_owned(display_data.clone());
                    let pixbuf = gdk_pixbuf::Pixbuf::from_bytes(
                        &bytes,
                        gdk_pixbuf::Colorspace::Rgb,
                        false,
                        8,
                        frame.width,
                        frame.height,
                        stride as i32,
                    );
                    let texture = gdk::Texture::for_pixbuf(&pixbuf);
                    view.image.set_paintable(Some(&texture));

                    *view.last_frame.borrow_mut() =
                        Some((display_data, frame.width as usize, frame.height as usize));
                    view.frame_count.fetch_add(1, Ordering::Relaxed);

                    // Collect callback info while holding the lock, but don't call
                    // it here. Capture an *owned* copy of the frame so the buffer
                    // outlives this tick and is valid for the whole deferred call.
                    if let Some(callback) = *view.frame_callback.lock().unwrap() {
                        let last_frame = view.last_frame.borrow();
                        if let Some((data, w, h)) = last_frame.as_ref() {
                            if *w > 0 && *h > 0 && !data.is_empty() {
                                pending_callback = Some((callback, data.clone(), *w, *h));
                            }
                        }
                    }
                }

                view.is_running.load(Ordering::Relaxed)
            });

            // Defer the callback to the GTK idle handler so the CAMERA_VIEWS lock
            // is released before (potentially re-entrant) JS runs. The owned
            // `frame_bytes` is moved into the closure and kept alive for the whole
            // synchronous call — same buffer-lifetime contract as the audio path.
            if let Some((callback, frame_bytes, w, h)) = pending_callback {
                gtk4::glib::idle_add_once(move || unsafe {
                    let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
                    js_closure_call3(
                        closure_ptr,
                        js_nanbox_pointer(frame_bytes.as_ptr() as i64),
                        w as f64,
                        h as f64,
                    );
                    // `frame_bytes` dropped here, after the JS call returns.
                });
            }

            is_running
        });

        match should_continue {
            Ok(val) => val.into(),
            Err(_) => false.into(),
        }
    });
}

fn yuy2_to_rgb(data: &[u8], width: usize, _height: usize) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(width * _height * 3);
    let mut i = 0;
    while i < data.len() {
        let y1 = data[i] as f64;
        let u = data[i + 1] as f64 - 128.0;
        let y2 = data[i + 2] as f64;
        let v = data[i + 3] as f64 - 128.0;

        let r1 = (y1 + 1.402 * v).max(0.0).min(255.0) as u8;
        let g1 = (y1 - 0.344 * u - 0.714 * v).max(0.0).min(255.0) as u8;
        let b1 = (y1 + 1.772 * u).max(0.0).min(255.0) as u8;

        let r2 = (y2 + 1.402 * v).max(0.0).min(255.0) as u8;
        let g2 = (y2 - 0.344 * u - 0.714 * v).max(0.0).min(255.0) as u8;
        let b2 = (y2 + 1.772 * u).max(0.0).min(255.0) as u8;

        rgb.push(r1);
        rgb.push(g1);
        rgb.push(b1);
        rgb.push(r2);
        rgb.push(g2);
        rgb.push(b2);

        i += 4;
    }
    rgb
}

fn bgr_to_rgb(data: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let g = data[i + 1];
        let r = data[i + 2];
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
        i += 3;
    }
    rgb
}

fn bgra_to_rgb(data: &[u8], _width: usize, _height: usize, format: &str) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(_width * _height * 3);
    let bytes_per_pixel = 4;

    let mut i = 0;
    while i < data.len() {
        let (b, g, r) = if format == "BGRA" {
            (data[i], data[i + 1], data[i + 2])
        } else {
            (data[i + 1], data[i + 2], data[i + 3])
        };
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
        i += bytes_per_pixel;
    }
    rgb
}

fn rgb16_to_rgb(data: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(data.len() * 3 / 2);
    let mut i = 0;
    while i + 1 < data.len() {
        let hi = data[i] as u16;
        let lo = data[i + 1] as u16;
        let rgb565 = (hi << 8) | lo;

        let r = ((rgb565 >> 11) & 0x1F) << 3;
        let g = ((rgb565 >> 5) & 0x3F) << 2;
        let b = (rgb565 & 0x1F) << 3;

        rgb.push(r as u8);
        rgb.push(g as u8);
        rgb.push(b as u8);
        i += 2;
    }
    rgb
}

fn nv12_to_rgb(data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(width * height * 3);
    let y_size = width * height;
    let uv_size = width * height / 2;

    if data.len() < y_size + uv_size {
        return rgb;
    }

    let y_plane = &data[0..y_size];
    let uv_plane = &data[y_size..y_size + uv_size];

    for y in 0..height {
        for x in 0..width {
            let y_val = y_plane[y * width + x] as f64;
            let uv_idx = (y / 2) * width + (x & !1);
            if uv_idx + 1 >= uv_plane.len() {
                continue;
            }
            let u = uv_plane[uv_idx] as f64 - 128.0;
            let v = uv_plane[uv_idx + 1] as f64 - 128.0;

            let r = (y_val + 1.402 * v).max(0.0).min(255.0) as u8;
            let g = (y_val - 0.344 * u - 0.714 * v).max(0.0).min(255.0) as u8;
            let b = (y_val + 1.772 * u).max(0.0).min(255.0) as u8;

            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
    }
    rgb
}

fn i420_to_rgb(data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(width * height * 3);
    let y_size = width * height;
    let u_size = width * height / 4;
    let v_size = width * height / 4;

    if data.len() < y_size + u_size + v_size {
        return rgb;
    }

    let y_plane = &data[0..y_size];
    let u_plane = &data[y_size..y_size + u_size];
    let v_plane = &data[y_size + u_size..y_size + u_size + v_size];

    for y in 0..height {
        for x in 0..width {
            let y_val = y_plane[y * width + x] as f64;
            let u = u_plane[(y / 2) * (width / 2) + (x / 2)] as f64 - 128.0;
            let v = v_plane[(y / 2) * (width / 2) + (x / 2)] as f64 - 128.0;

            let r = (y_val + 1.402 * v).max(0.0).min(255.0) as u8;
            let g = (y_val - 0.344 * u - 0.714 * v).max(0.0).min(255.0) as u8;
            let b = (y_val + 1.772 * u).max(0.0).min(255.0) as u8;

            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
    }
    rgb
}

fn create_pipeline(
    _handle: i64,
    sender: mpsc::Sender<FrameData>,
) -> Result<gstreamer::Pipeline, String> {
    ensure_gst_init()?;

    if !camera_device_exists() {
        return Err("No camera device found".to_string());
    }

    let pipeline = gstreamer::Pipeline::new();

    let src = gstreamer::ElementFactory::make("v4l2src")
        .build()
        .map_err(|e| format!("Failed to create v4l2src element: {:?}", e))?;

    let videoconvert = gstreamer::ElementFactory::make("videoconvert")
        .build()
        .map_err(|e| format!("Failed to create videoconvert element: {:?}", e))?;

    let appsink = gstreamer::ElementFactory::make("appsink")
        .build()
        .map_err(|e| format!("Failed to create appsink element: {:?}", e))?;
    appsink.set_property("emit-signals", true);
    appsink.set_property("sync", false);

    pipeline
        .add_many(&[&src, &videoconvert, &appsink])
        .map_err(|e| format!("Failed to add elements to pipeline: {:?}", e))?;

    src.link(&videoconvert)
        .map_err(|e| format!("Failed to link src to videoconvert: {:?}", e))?;
    videoconvert
        .link(&appsink)
        .map_err(|e| format!("Failed to link videoconvert to appsink: {:?}", e))?;

    let appsink_elem = appsink.clone();
    let appsink = gstreamer::prelude::Cast::dynamic_cast::<gstreamer_app::AppSink>(appsink_elem)
        .map_err(|_| "Failed to cast appsink to AppSink")?;

    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                let sample = appsink
                    .pull_sample()
                    .map_err(|_| gstreamer::FlowError::Eos)?;
                let buffer = sample.buffer().ok_or(gstreamer::FlowError::Eos)?;
                let caps = sample.caps().ok_or(gstreamer::FlowError::Eos)?;
                let structure = caps.structure(0).ok_or(gstreamer::FlowError::Eos)?;

                let width = structure.get::<i32>("width").unwrap_or(640);
                let height = structure.get::<i32>("height").unwrap_or(480);
                let format = structure
                    .get::<&str>("format")
                    .unwrap_or("UNKNOWN")
                    .to_string();
                let stride = structure.get::<i32>("stride").unwrap_or(width * 3);

                let map = buffer
                    .map_readable()
                    .map_err(|_| gstreamer::FlowError::Eos)?;
                let frame_data = map.as_slice().to_vec();

                let _ = sender.send(FrameData {
                    data: frame_data,
                    width,
                    height,
                    format,
                    stride,
                });

                Ok(gstreamer::FlowSuccess::Ok)
            })
            .build(),
    );

    pipeline
        .set_state(gstreamer::State::Playing)
        .map_err(|e| format!("Failed to start pipeline: {:?}", e))?;

    Ok(pipeline)
}

fn camera_device_exists() -> bool {
    use std::fs;
    let dev_dir = "/dev";

    if let Ok(entries) = fs::read_dir(dev_dir) {
        for entry in entries.flatten() {
            if let Some(file_name) = entry.file_name().to_str() {
                if file_name.starts_with("video") {
                    return true;
                }
            }
        }
    }
    false
}

pub fn stop(handle: i64) {
    CAMERA_VIEWS.with(|c| {
        let mut views = c.lock().unwrap();
        let Some(view) = views.get_mut(&handle) else {
            return;
        };

        view.is_running.store(false, Ordering::Relaxed);

        if let Some(pipeline) = view.pipeline.take() {
            pipeline.set_state(gstreamer::State::Null).ok();
        }

        view.is_frozen.store(false, Ordering::Relaxed);
    });
}

pub fn freeze(handle: i64) {
    CAMERA_VIEWS.with(|c| {
        let views = c.lock().unwrap();
        if let Some(view) = views.get(&handle) {
            view.is_frozen.store(true, Ordering::Relaxed);
        }
    });
}

pub fn unfreeze(handle: i64) {
    CAMERA_VIEWS.with(|c| {
        let views = c.lock().unwrap();
        if let Some(view) = views.get(&handle) {
            view.is_frozen.store(false, Ordering::Relaxed);
        }
    });
}

pub fn sample_color(x: f64, y: f64) -> f64 {
    CAMERA_VIEWS.with(|c| {
        let views = c.lock().unwrap();
        for view in views.values() {
            if view.is_running.load(Ordering::Relaxed) && !view.is_frozen.load(Ordering::Relaxed) {
                if let Some((data, width, height)) = view.last_frame.borrow().as_ref() {
                    let px = (x * *width as f64) as usize;
                    let py = (y * *height as f64) as usize;

                    if px < *width && py < *height {
                        let idx = (py * *width + px) * 3;
                        if idx + 2 < data.len() {
                            let r = data[idx] as f64;
                            let g = data[idx + 1] as f64;
                            let b = data[idx + 2] as f64;
                            return r * 65536.0 + g * 256.0 + b;
                        }
                    }
                }
            }
        }
        -1.0
    })
}

pub fn set_on_tap(_handle: i64, _callback: f64) {}

pub fn register_frame_callback(handle: i64, callback: f64) {
    CAMERA_VIEWS.with(|c| {
        let mut views = c.lock().unwrap();
        if let Some(view) = views.get_mut(&handle) {
            *view.frame_callback.lock().unwrap() = Some(callback);
        }
    });
}

pub fn unregister_frame_callback(handle: i64) {
    CAMERA_VIEWS.with(|c| {
        let mut views = c.lock().unwrap();
        if let Some(view) = views.get_mut(&handle) {
            *view.frame_callback.lock().unwrap() = None;
        }
    });
}
