//! iOS game loop support.
//!
//! When a Perry app targets iOS with `--features ios-game-loop`, the compiler
//! generates `_perry_user_main` instead of `main`. This module provides the
//! actual `main` entry point which:
//!
//! 1. Spawns `_perry_user_main` on a background "game thread"
//! 2. Calls `UIApplicationMain` on the main thread (required by UIKit)
//!
//! This allows game-loop style apps (which block in a `while` loop) to work
//! on iOS where `UIApplicationMain` must own the main thread.

use std::ffi::c_void;

extern "C" {
    fn _perry_user_main() -> i32;

    fn UIApplicationMain(
        argc: i32,
        argv: *const *const u8,
        principalClassName: *const c_void,
        delegateClassName: *const c_void,
    ) -> i32;

    /// Provided by native libraries (e.g., Bloom Engine) to register ObjC classes
    /// (like scene delegates) before UIApplicationMain starts.
    fn perry_register_native_classes();

    /// Called when the UIWindowScene connects. Native libraries implement this
    /// to create their window, Metal view, and wgpu surface.
    fn perry_scene_will_connect(scene: *const c_void);
}

/// Global: set to the UIWindowScene pointer when scene connects
static CONNECTED_SCENE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Check if a scene has connected (returns scene pointer or 0)
#[no_mangle]
pub extern "C" fn perry_ios_get_connected_scene() -> u64 {
    CONNECTED_SCENE.load(std::sync::atomic::Ordering::Acquire)
}

/// Fallback scene delegate — creates a window with the native lib's view class,
/// stores references so the game thread can create the wgpu surface.
unsafe extern "C" fn fallback_scene_will_connect(
    _this: *mut c_void,
    _sel: *const c_void,
    scene: *const c_void,
    _session: *const c_void,
    _options: *const c_void,
) {
    if scene.is_null() {
        return;
    }
    CONNECTED_SCENE.store(scene as u64, std::sync::atomic::Ordering::Release);

    // Call the native library's scene setup function if it exists
    perry_scene_will_connect(scene);
}

fn register_fallback_scene_delegate() {
    extern "C" {
        fn objc_getClass(name: *const u8) -> *const c_void;
        fn objc_allocateClassPair(
            superclass: *const c_void,
            name: *const u8,
            extra: usize,
        ) -> *mut c_void;
        fn objc_registerClassPair(cls: *mut c_void);
        fn class_addMethod(
            cls: *mut c_void,
            sel: *const c_void,
            imp: *const c_void,
            types: *const u8,
        ) -> bool;
        fn sel_registerName(name: *const u8) -> *const c_void;
        fn objc_getProtocol(name: *const u8) -> *const c_void;
        fn class_addProtocol(cls: *mut c_void, protocol: *const c_void) -> bool;
    }

    unsafe {
        // Only register if the native lib didn't already register it
        let existing = objc_getClass(b"PerrySceneDelegate\0".as_ptr());
        if !existing.is_null() {
            return;
        }

        let superclass = objc_getClass(b"UIResponder\0".as_ptr());
        if superclass.is_null() {
            return;
        }

        let cls = objc_allocateClassPair(superclass, b"PerrySceneDelegate\0".as_ptr(), 0);
        if cls.is_null() {
            return;
        }

        let sel = sel_registerName(b"scene:willConnectToSession:connectionOptions:\0".as_ptr());
        class_addMethod(
            cls,
            sel,
            fallback_scene_will_connect as *const c_void,
            b"v48@0:8@16@24@32\0".as_ptr(),
        );

        let protocol = objc_getProtocol(b"UIWindowSceneDelegate\0".as_ptr());
        if !protocol.is_null() {
            class_addProtocol(cls, protocol);
        }

        objc_registerClassPair(cls);
    }
}

/// App delegate — calls perry_scene_will_connect to create the window.
/// We pass the scene as null; the native lib creates the window without a scene.
unsafe extern "C" fn did_finish_launching(
    _this: *mut c_void,
    _sel: *const c_void,
    _app: *const c_void,
    _opts: *const c_void,
) -> bool {
    // Create window directly from didFinishLaunching (no scene lifecycle)
    perry_scene_will_connect(std::ptr::null());
    true
}

fn register_app_delegate() {
    extern "C" {
        fn objc_getClass(name: *const u8) -> *const c_void;
        fn objc_allocateClassPair(
            superclass: *const c_void,
            name: *const u8,
            extra: usize,
        ) -> *mut c_void;
        fn objc_registerClassPair(cls: *mut c_void);
        fn class_addMethod(
            cls: *mut c_void,
            sel: *const c_void,
            imp: *const c_void,
            types: *const u8,
        ) -> bool;
        fn sel_registerName(name: *const u8) -> *const c_void;
        fn objc_getProtocol(name: *const u8) -> *const c_void;
        fn class_addProtocol(cls: *mut c_void, protocol: *const c_void) -> bool;
    }

    unsafe {
        // Check if already registered
        let existing = objc_getClass(b"PerryGameLoopAppDelegate\0".as_ptr());
        if !existing.is_null() {
            return;
        }

        let superclass = objc_getClass(b"UIResponder\0".as_ptr());
        if superclass.is_null() {
            return;
        }

        let cls = objc_allocateClassPair(superclass, b"PerryGameLoopAppDelegate\0".as_ptr(), 0);
        if cls.is_null() {
            return;
        }

        // Add application:didFinishLaunchingWithOptions:
        let sel = sel_registerName(b"application:didFinishLaunchingWithOptions:\0".as_ptr());
        class_addMethod(
            cls,
            sel,
            did_finish_launching as *const c_void,
            b"B32@0:8@16@24\0".as_ptr(),
        );

        // Add UIApplicationDelegate protocol
        let protocol = objc_getProtocol(b"UIApplicationDelegate\0".as_ptr());
        if !protocol.is_null() {
            class_addProtocol(cls, protocol);
        }

        objc_registerClassPair(cls);
    }
}

/// Called by native libraries (e.g., Bloom Engine) to register their scene
/// delegate class before UIApplicationMain creates the scene session.
/// Must be called from the game thread during bloom_init_window or similar.
static NATIVE_CLASSES_REGISTERED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Wait for native library classes (like PerrySceneDelegate) to be registered.
fn wait_for_native_classes() {
    // The game thread registers ObjC classes during its init.
    // We need to wait for that before calling UIApplicationMain.
    for _ in 0..500 {
        if NATIVE_CLASSES_REGISTERED.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Timeout — proceed anyway, scene delegate may already be registered
}

/// Called by native libraries after registering their ObjC classes.
#[no_mangle]
pub extern "C" fn perry_ios_classes_registered() {
    NATIVE_CLASSES_REGISTERED.store(true, std::sync::atomic::Ordering::Release);
}

/// The actual `main` entry point for iOS game loop apps.
/// Spawns the user's code on a game thread, then enters UIApplicationMain.
#[no_mangle]
pub extern "C" fn main() -> i32 {
    register_app_delegate();

    // Spawn the game thread — runs the user's TypeScript code
    std::thread::Builder::new()
        .name("perry-game".to_string())
        .spawn(|| unsafe {
            _perry_user_main();
        })
        .expect("Failed to spawn game thread");

    // Register native library ObjC classes (e.g., PerrySceneDelegate with scene_will_connect)
    unsafe {
        perry_register_native_classes();
    }

    // Also register PerrySceneDelegate if it doesn't exist yet (fallback)
    register_fallback_scene_delegate();

    // Main thread: enter UIApplicationMain (blocks forever)
    // The native library (e.g., Bloom Engine) must register its own
    // PerrySceneDelegate to handle window/scene creation.
    unsafe {
        // Create NSString via CFStringCreateWithCString (toll-free bridged to NSString)
        extern "C" {
            fn CFStringCreateWithCString(
                alloc: *const c_void,
                cstr: *const u8,
                encoding: u32,
            ) -> *const c_void;
        }
        const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

        let delegate_name = CFStringCreateWithCString(
            std::ptr::null(),
            b"PerryGameLoopAppDelegate\0".as_ptr(),
            K_CF_STRING_ENCODING_UTF8,
        );

        // Read NSPrincipalClass from Info.plist for custom UIApplication subclass
        // (e.g., BloomApplication on tvOS which overrides sendEvent: for input)
        extern "C" {
            fn objc_getClass(name: *const u8) -> *const c_void;
            fn objc_msgSend();
            fn sel_registerName(name: *const u8) -> *const c_void;
        }
        let mut principal_class: *const c_void = std::ptr::null();
        let bundle_cls = objc_getClass(b"NSBundle\0".as_ptr());
        if !bundle_cls.is_null() {
            let main_bundle: unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let sel_main = sel_registerName(b"mainBundle\0".as_ptr());
            let bundle = main_bundle(bundle_cls, sel_main);
            if !bundle.is_null() {
                let info_dict: unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void =
                    std::mem::transmute(objc_msgSend as *const c_void);
                let sel_info = sel_registerName(b"infoDictionary\0".as_ptr());
                let dict = info_dict(bundle, sel_info);
                if !dict.is_null() {
                    let key = CFStringCreateWithCString(
                        std::ptr::null(),
                        b"NSPrincipalClass\0".as_ptr(),
                        K_CF_STRING_ENCODING_UTF8,
                    );
                    let obj_for_key: unsafe extern "C" fn(
                        *const c_void,
                        *const c_void,
                        *const c_void,
                    ) -> *const c_void = std::mem::transmute(objc_msgSend as *const c_void);
                    let sel_obj = sel_registerName(b"objectForKey:\0".as_ptr());
                    let val = obj_for_key(dict, sel_obj, key);
                    if !val.is_null() {
                        principal_class = val; // NSString of the class name
                    }
                }
            }
        }

        UIApplicationMain(0, std::ptr::null(), principal_class, delegate_name);
    }

    0
}
