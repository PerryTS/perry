// Camera widget — live camera preview with color sampling
//
// Uses AVCaptureSession + AVCaptureVideoPreviewLayer for live preview,
// AVCaptureVideoDataOutput for frame capture, and CVPixelBuffer for color sampling.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, Encode, Encoding, RefEncode};
use objc2_ui_kit::UIView;
use objc2_core_foundation::CGRect;
use std::cell::RefCell;
use std::sync::atomic::{AtomicPtr, AtomicBool, Ordering};
use std::ffi::c_void;

use crate::widgets;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_stdlib_process_pending();
    fn js_promise_run_microtasks() -> i32;
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
}

// Core Video / Core Media C API
type CVPixelBufferRef = *mut c_void;
type CVReturn = i32;

extern "C" {
    fn CVPixelBufferLockBaseAddress(pixelBuffer: CVPixelBufferRef, lockFlags: u64) -> CVReturn;
    fn CVPixelBufferUnlockBaseAddress(pixelBuffer: CVPixelBufferRef, lockFlags: u64) -> CVReturn;
    fn CVPixelBufferGetBaseAddress(pixelBuffer: CVPixelBufferRef) -> *mut u8;
    fn CVPixelBufferGetWidth(pixelBuffer: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetHeight(pixelBuffer: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetBytesPerRow(pixelBuffer: CVPixelBufferRef) -> usize;
    fn CMSampleBufferGetImageBuffer(sbuf: *mut c_void) -> CVPixelBufferRef;
    fn CFRetain(cf: *mut c_void) -> *mut c_void;
    fn CFRelease(cf: *mut c_void);
}

// ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(
        superclass: *const c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut c_void;
    fn objc_registerClassPair(cls: *mut c_void);
    fn class_addMethod(
        cls: *mut c_void,
        sel: *const c_void,
        imp: *const c_void,
        types: *const i8,
    ) -> bool;
    fn sel_registerName(name: *const i8) -> *const c_void;
    fn objc_getClass(name: *const i8) -> *const c_void;
}

// =============================================================================
// State
// =============================================================================

/// Latest CMSampleBuffer — written by capture delegate (background queue),
/// read by sample_color (main thread). Uses AtomicPtr for thread safety.
static LATEST_BUFFER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static FROZEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    static CAPTURE_SESSION: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static PREVIEW_LAYER: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static CAMERA_VIEW: RefCell<Option<Retained<UIView>>> = RefCell::new(None);
    static DELEGATE_INSTANCE: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static DELEGATE_REGISTERED: RefCell<bool> = RefCell::new(false);
    static TAP_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static TAP_TARGET: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
}

// =============================================================================
// Camera delegate — receives frames from AVCaptureVideoDataOutput
// =============================================================================

/// captureOutput:didOutputSampleBuffer:fromConnection:
unsafe extern "C" fn did_output_sample_buffer(
    _this: *mut AnyObject,
    _sel: *const c_void,
    _output: *mut AnyObject,
    sample_buffer: *mut c_void,
    _connection: *mut AnyObject,
) {
    if FROZEN.load(Ordering::Relaxed) {
        return;
    }
    if sample_buffer.is_null() {
        return;
    }
    // Retain the new buffer and swap with the old one
    let new_buf = CFRetain(sample_buffer);
    let old_buf = LATEST_BUFFER.swap(new_buf, Ordering::Release);
    if !old_buf.is_null() {
        CFRelease(old_buf);
    }
}

fn register_camera_delegate() {
    DELEGATE_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;

        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryCameraDelegate".as_ptr(), 0);
            if cls.is_null() {
                return; // Already registered
            }

            // captureOutput:didOutputSampleBuffer:fromConnection: — type: v@:@@@
            let sel = sel_registerName(c"captureOutput:didOutputSampleBuffer:fromConnection:".as_ptr());
            class_addMethod(
                cls, sel,
                did_output_sample_buffer as *const c_void,
                c"v@:@@@".as_ptr(),
            );

            objc_registerClassPair(cls);
        }
    });
}

// =============================================================================
// Camera view — UIView subclass with layoutSubviews override
// =============================================================================

/// Register PerryCameraView class (UIView that resizes its preview sublayer)
fn camera_view_class() -> *const c_void {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    static mut CLS: *const c_void = std::ptr::null();
    REGISTER.call_once(|| {
        unsafe {
            let superclass = objc_getClass(c"UIView".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryCameraView".as_ptr(), 0);
            assert!(!cls.is_null(), "Failed to allocate PerryCameraView class");

            extern "C" fn layout_subviews(this: *mut AnyObject, _cmd: *const c_void) {
                unsafe {
                    // Call [super layoutSubviews]
                    let sup = AnyClass::get(c"UIView").unwrap();
                    let _: () = msg_send![super(this, sup), layoutSubviews];
                    // Resize all sublayers to match layer bounds
                    let layer: *mut AnyObject = msg_send![this, layer];
                    if layer.is_null() { return; }
                    let bounds: CGRect = msg_send![layer, bounds];
                    let sublayers: *mut AnyObject = msg_send![layer, sublayers];
                    if !sublayers.is_null() {
                        let count: usize = msg_send![sublayers, count];
                        for i in 0..count {
                            let sub: *mut AnyObject = msg_send![sublayers, objectAtIndex: i];
                            let _: () = msg_send![sub, setFrame: bounds];
                        }
                    }
                }
            }

            let sel = sel_registerName(c"layoutSubviews".as_ptr());
            class_addMethod(cls, sel, layout_subviews as *const c_void, c"v@:".as_ptr());

            objc_registerClassPair(cls);
            CLS = cls as *const c_void;
        }
    });
    unsafe { CLS }
}

// =============================================================================
// Tap handler — reports normalized (x, y) coordinates
// =============================================================================

fn register_tap_handler_class() -> *const c_void {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    static mut CLS: *const c_void = std::ptr::null();
    REGISTER.call_once(|| {
        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryCameraTapHandler".as_ptr(), 0);
            assert!(!cls.is_null());

            extern "C" fn handle_tap(this: *mut AnyObject, _cmd: *const c_void, recognizer: *mut AnyObject) {
                unsafe {
                    // Get the view from the gesture recognizer
                    let view: *mut AnyObject = msg_send![recognizer, view];
                    if view.is_null() { return; }

                    // Get location in view
                    #[repr(C)]
                    #[derive(Copy, Clone)]
                    struct CGPoint { x: f64, y: f64 }
                    unsafe impl Encode for CGPoint {
                        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
                    }
                    unsafe impl RefEncode for CGPoint {
                        const ENCODING_REF: Encoding = Encoding::Pointer(&<Self as Encode>::ENCODING);
                    }

                    let point: CGPoint = msg_send![recognizer, locationInView: view];
                    let bounds: CGRect = msg_send![view, bounds];

                    // Normalize to 0-1
                    let norm_x = if bounds.size.width > 0.0 { point.x / bounds.size.width } else { 0.5 };
                    let norm_y = if bounds.size.height > 0.0 { point.y / bounds.size.height } else { 0.5 };

                    // Dispatch callback on main queue
                    TAP_CALLBACK.with(|cb| {
                        if let Some(closure_f64) = *cb.borrow() {
                            js_stdlib_process_pending();
                            js_promise_run_microtasks();
                            let closure_ptr = js_nanbox_get_pointer(closure_f64);
                            js_closure_call2(closure_ptr as *const u8, norm_x, norm_y);
                        }
                    });
                }
            }

            let sel = sel_registerName(c"handleTap:".as_ptr());
            class_addMethod(cls, sel, handle_tap as *const c_void, c"v@:@".as_ptr());

            objc_registerClassPair(cls);
            CLS = cls as *const c_void;
        }
    });
    unsafe { CLS }
}

// =============================================================================
// Public API
// =============================================================================

/// Create a CameraView widget. Returns widget handle.
pub fn create() -> i64 {
    let cls = camera_view_class();
    unsafe {
        let view: *mut AnyObject = msg_send![cls as *const AnyObject, new];
        let _: () = msg_send![view, setTranslatesAutoresizingMaskIntoConstraints: false];
        let _: () = msg_send![view, setClipsToBounds: true];

        // Set black background
        let black: Retained<AnyObject> = msg_send![AnyClass::get(c"UIColor").unwrap(), blackColor];
        let _: () = msg_send![view, setBackgroundColor: &*black];

        let retained: Retained<UIView> = Retained::retain(view as *mut UIView).unwrap();

        // Store for later access
        CAMERA_VIEW.with(|cv| {
            *cv.borrow_mut() = Some(retained.clone());
        });

        widgets::register_widget(retained)
    }
}

/// Start the camera capture session. Handles permissions internally.
pub fn start(_handle: i64) {
    register_camera_delegate();

    let already_running = CAPTURE_SESSION.with(|s| s.borrow().is_some());
    if already_running {
        return;
    }

    unsafe {
        // Check camera authorization
        let device_cls = AnyClass::get(c"AVCaptureDevice")
            .expect("AVCaptureDevice not found — link AVFoundation.framework");

        // AVMediaTypeVideo
        let media_type = objc2_foundation::NSString::from_str("vide");
        let status: i64 = msg_send![device_cls, authorizationStatusForMediaType: &*media_type];
        // 0 = notDetermined, 1 = restricted, 2 = denied, 3 = authorized
        if status == 2 || status == 1 {
            crate::ws_log!("[camera] permission denied/restricted");
            return;
        }
        if status == 0 {
            // Request permission — user calls start() again after granting
            let permission_block = block2::RcBlock::new(|_granted: objc2::runtime::Bool| {
                // Permission result handled on next start() call
            });
            let _: () = msg_send![device_cls, requestAccessForMediaType: &*media_type completionHandler: &*permission_block];
            crate::ws_log!("[camera] requesting camera permission");
            return;
        }

        // Create AVCaptureSession
        let session_cls = AnyClass::get(c"AVCaptureSession").unwrap();
        let session: Retained<AnyObject> = msg_send![session_cls, new];

        // Set preset
        let preset = objc2_foundation::NSString::from_str("AVCaptureSessionPresetHigh");
        let _: () = msg_send![&*session, setSessionPreset: &*preset];

        // Get default video device (back camera)
        let media_type = objc2_foundation::NSString::from_str("vide");
        let device: *mut AnyObject = msg_send![device_cls, defaultDeviceWithMediaType: &*media_type];
        if device.is_null() {
            crate::ws_log!("[camera] no video device found");
            return;
        }

        // Create input
        let input_cls = AnyClass::get(c"AVCaptureDeviceInput").unwrap();
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let input: *mut AnyObject = msg_send![input_cls, deviceInputWithDevice: device error: &mut error];
        if input.is_null() || !error.is_null() {
            crate::ws_log!("[camera] failed to create device input");
            return;
        }

        let can_add_input: bool = msg_send![&*session, canAddInput: input];
        if !can_add_input {
            crate::ws_log!("[camera] cannot add input to session");
            return;
        }
        let _: () = msg_send![&*session, addInput: input];

        // Create video data output
        let output_cls = AnyClass::get(c"AVCaptureVideoDataOutput").unwrap();
        let output: Retained<AnyObject> = msg_send![output_cls, new];

        // Set pixel format to BGRA
        let pixel_format_key = objc2_foundation::NSString::from_str("PixelFormatType");
        let bgra_format: i64 = 0x42475241; // 'BGRA' = kCVPixelFormatType_32BGRA
        let format_num: Retained<AnyObject> = msg_send![
            AnyClass::get(c"NSNumber").unwrap(),
            numberWithInteger: bgra_format
        ];
        let settings_cls = AnyClass::get(c"NSDictionary").unwrap();
        let video_settings: Retained<AnyObject> = msg_send![
            settings_cls,
            dictionaryWithObject: &*format_num,
            forKey: &*pixel_format_key
        ];
        let _: () = msg_send![&*output, setVideoSettings: &*video_settings];
        let _: () = msg_send![&*output, setAlwaysDiscardsLateVideoFrames: true];

        // Create delegate
        let del_cls = AnyClass::get(c"PerryCameraDelegate").unwrap();
        let delegate: Retained<AnyObject> = msg_send![del_cls, new];

        // Create serial dispatch queue for delegate
        let queue_label = c"com.perry.camera.queue";
        extern "C" {
            fn dispatch_queue_create(label: *const i8, attr: *const c_void) -> *mut c_void;
        }
        let queue = dispatch_queue_create(queue_label.as_ptr(), std::ptr::null());

        let _: () = msg_send![&*output, setSampleBufferDelegate: &*delegate queue: queue];

        let can_add_output: bool = msg_send![&*session, canAddOutput: &*output];
        if !can_add_output {
            crate::ws_log!("[camera] cannot add output to session");
            return;
        }
        let _: () = msg_send![&*session, addOutput: &*output];

        // Create preview layer
        let preview_cls = AnyClass::get(c"AVCaptureVideoPreviewLayer").unwrap();
        let preview: Retained<AnyObject> = msg_send![preview_cls, layerWithSession: &*session];

        // Set video gravity to aspect fill
        let gravity = objc2_foundation::NSString::from_str("AVLayerVideoGravityResizeAspectFill");
        let _: () = msg_send![&*preview, setVideoGravity: &*gravity];

        // Add preview layer to the camera view
        CAMERA_VIEW.with(|cv| {
            if let Some(view) = cv.borrow().as_ref() {
                let layer: *mut AnyObject = msg_send![&**view, layer];
                let _: () = msg_send![layer, addSublayer: &*preview];
            }
        });

        // Start session on a background thread to avoid blocking UI
        let session_ptr = Retained::as_ptr(&session) as usize;
        let start_block = block2::RcBlock::new(move || {
            let session = session_ptr as *mut AnyObject;
            let _: () = msg_send![session, startRunning];
        });

        extern "C" {
            fn dispatch_async(queue: *mut c_void, block: *const c_void);
        }
        extern "C" {
            fn dispatch_get_global_queue(identifier: i64, flags: u64) -> *mut c_void;
        }
        let global_queue = dispatch_get_global_queue(0, 0); // QOS_CLASS_DEFAULT
        dispatch_async(global_queue, &*start_block as *const _ as *const c_void);

        // Store session, preview layer, and delegate
        CAPTURE_SESSION.with(|s| { *s.borrow_mut() = Some(session); });
        PREVIEW_LAYER.with(|p| { *p.borrow_mut() = Some(preview); });
        DELEGATE_INSTANCE.with(|d| { *d.borrow_mut() = Some(delegate); });

        // Update video orientation to match current device orientation
        update_video_orientation();

        // Register for orientation change notifications so preview stays correct on iPad rotation
        let nc: *mut AnyObject = msg_send![AnyClass::get(c"NSNotificationCenter").unwrap(), defaultCenter];
        let device_cls = AnyClass::get(c"UIDevice").unwrap();
        let current_device: *mut AnyObject = msg_send![device_cls, currentDevice];
        let _: () = msg_send![current_device, beginGeneratingDeviceOrientationNotifications];

        // Use a block-based observer for orientation changes
        let name = objc2_foundation::NSString::from_str("UIDeviceOrientationDidChangeNotification");
        let observer_block = block2::RcBlock::new(move |_notif: *mut AnyObject| {
            update_video_orientation();
        });
        let _: *mut AnyObject = msg_send![nc,
            addObserverForName: &*name,
            object: std::ptr::null::<AnyObject>(),
            queue: std::ptr::null::<AnyObject>(),  // nil = deliver on posting queue
            usingBlock: &*observer_block
        ];

        crate::ws_log!("[camera] session started");
    }
}

/// Update the preview layer's connection videoOrientation to match current device orientation.
fn update_video_orientation() {
    unsafe {
        PREVIEW_LAYER.with(|p| {
            if let Some(preview) = p.borrow().as_ref() {
                let connection: *mut AnyObject = msg_send![&**preview, connection];
                if connection.is_null() { return; }

                let device_cls = AnyClass::get(c"UIDevice").unwrap();
                let current_device: *mut AnyObject = msg_send![device_cls, currentDevice];
                let device_orientation: i64 = msg_send![current_device, orientation];

                // Map UIDeviceOrientation to AVCaptureVideoOrientation
                // UIDeviceOrientation: 1=portrait, 2=portraitUpsideDown, 3=landscapeLeft, 4=landscapeRight
                // AVCaptureVideoOrientation: 1=portrait, 2=portraitUpsideDown, 3=landscapeRight, 4=landscapeLeft
                let video_orientation: i64 = match device_orientation {
                    1 => 1, // Portrait
                    2 => 2, // PortraitUpsideDown
                    3 => 4, // LandscapeLeft device → LandscapeLeft video (AVCapture swaps L/R)
                    4 => 3, // LandscapeRight device → LandscapeRight video
                    _ => return, // Unknown/faceUp/faceDown — keep current
                };

                let _: () = msg_send![connection, setVideoOrientation: video_orientation];
            }
        });
    }
}

/// Stop the camera capture session.
pub fn stop(_handle: i64) {
    CAPTURE_SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().take() {
            unsafe {
                let _: () = msg_send![&*session, stopRunning];
            }
            crate::ws_log!("[camera] session stopped");
        }
    });
    // Release the latest buffer
    let old = LATEST_BUFFER.swap(std::ptr::null_mut(), Ordering::Release);
    if !old.is_null() {
        unsafe { CFRelease(old); }
    }
    FROZEN.store(false, Ordering::Relaxed);
}

/// Freeze the camera (stop updating the buffer, pause preview).
pub fn freeze(_handle: i64) {
    FROZEN.store(true, Ordering::Relaxed);
    // Pause the preview layer connection
    PREVIEW_LAYER.with(|p| {
        if let Some(preview) = p.borrow().as_ref() {
            unsafe {
                let connection: *mut AnyObject = msg_send![&**preview, connection];
                if !connection.is_null() {
                    let _: () = msg_send![connection, setEnabled: false];
                }
            }
        }
    });
    crate::ws_log!("[camera] frozen");
}

/// Unfreeze the camera (resume live capture).
pub fn unfreeze(_handle: i64) {
    FROZEN.store(false, Ordering::Relaxed);
    // Resume the preview layer connection
    PREVIEW_LAYER.with(|p| {
        if let Some(preview) = p.borrow().as_ref() {
            unsafe {
                let connection: *mut AnyObject = msg_send![&**preview, connection];
                if !connection.is_null() {
                    let _: () = msg_send![connection, setEnabled: true];
                }
            }
        }
    });
    crate::ws_log!("[camera] unfrozen");
}

/// Sample the color at normalized coordinates (0.0-1.0) from the latest frame.
/// Returns packed RGB as f64: r * 65536 + g * 256 + b.
/// Returns -1.0 if no frame is available.
pub fn sample_color(x: f64, y: f64) -> f64 {
    let buffer = LATEST_BUFFER.load(Ordering::Acquire);
    if buffer.is_null() {
        return -1.0;
    }

    unsafe {
        let pixel_buffer = CMSampleBufferGetImageBuffer(buffer);
        if pixel_buffer.is_null() {
            return -1.0;
        }

        // Lock the pixel buffer for read-only access
        let lock_result = CVPixelBufferLockBaseAddress(pixel_buffer, 1); // kCVPixelBufferLock_ReadOnly = 1
        if lock_result != 0 {
            return -1.0;
        }

        let base_address = CVPixelBufferGetBaseAddress(pixel_buffer);
        let width = CVPixelBufferGetWidth(pixel_buffer);
        let height = CVPixelBufferGetHeight(pixel_buffer);
        let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);

        if base_address.is_null() || width == 0 || height == 0 {
            CVPixelBufferUnlockBaseAddress(pixel_buffer, 1);
            return -1.0;
        }

        // Clamp coordinates
        let px = ((x.clamp(0.0, 1.0) * (width as f64 - 1.0)) as usize).min(width - 1);
        let py = ((y.clamp(0.0, 1.0) * (height as f64 - 1.0)) as usize).min(height - 1);

        // Average a 5x5 region around the sample point for noise reduction
        let half = 2usize;
        let mut r_sum: u64 = 0;
        let mut g_sum: u64 = 0;
        let mut b_sum: u64 = 0;
        let mut count: u64 = 0;

        let y_start = py.saturating_sub(half);
        let y_end = (py + half + 1).min(height);
        let x_start = px.saturating_sub(half);
        let x_end = (px + half + 1).min(width);

        for sy in y_start..y_end {
            for sx in x_start..x_end {
                let offset = sy * bytes_per_row + sx * 4;
                let ptr = base_address.add(offset);
                // BGRA format
                b_sum += *ptr as u64;
                g_sum += *ptr.add(1) as u64;
                r_sum += *ptr.add(2) as u64;
                count += 1;
            }
        }

        CVPixelBufferUnlockBaseAddress(pixel_buffer, 1);

        if count == 0 {
            return -1.0;
        }

        let r = (r_sum / count) as f64;
        let g = (g_sum / count) as f64;
        let b = (b_sum / count) as f64;

        r * 65536.0 + g * 256.0 + b
    }
}

/// Set a tap handler that receives normalized (x, y) coordinates.
pub fn set_on_tap(handle: i64, callback: f64) {
    TAP_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });

    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let tap_cls = register_tap_handler_class();
            let target: Retained<AnyObject> = msg_send![tap_cls as *const AnyObject, new];

            let sel = sel_registerName(c"handleTap:".as_ptr());
            let gr_cls = AnyClass::get(c"UITapGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = msg_send![
                recognizer, initWithTarget: &*target, action: sel as *const c_void as *const AnyObject
            ];
            let _: () = msg_send![recognizer, setNumberOfTapsRequired: 1i64];
            let _: () = msg_send![&*view, setUserInteractionEnabled: true];
            let _: () = msg_send![&*view, addGestureRecognizer: recognizer];

            // Keep target alive
            TAP_TARGET.with(|t| {
                *t.borrow_mut() = Some(target);
            });
        }
    }
}
