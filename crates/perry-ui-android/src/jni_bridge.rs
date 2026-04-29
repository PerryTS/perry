use jni::objects::{GlobalRef, JClass, JMethodID};
use jni::{JNIEnv, JavaVM};
use std::cell::RefCell;
use std::sync::OnceLock;

/// The global JavaVM reference, set once during JNI_OnLoad.
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

/// Cached JNI class and method references (thread-local because JNIEnv is per-thread).
/// These are populated lazily on first use per thread.
pub struct JniCache {
    // --- Classes ---
    pub text_view_class: GlobalRef,
    pub button_class: GlobalRef,
    pub linear_layout_class: GlobalRef,
    pub scroll_view_class: GlobalRef,
    pub edit_text_class: GlobalRef,
    pub switch_class: GlobalRef,
    pub seek_bar_class: GlobalRef,
    pub space_class: GlobalRef,
    pub view_class: GlobalRef,
    pub color_class: GlobalRef,
    pub typeface_class: GlobalRef,
    pub popup_menu_class: GlobalRef,
    pub clipboard_manager_class: GlobalRef,
    pub context_class: GlobalRef,
    pub frame_layout_class: GlobalRef,
    pub view_group_class: GlobalRef,
    pub view_group_layout_params_class: GlobalRef,
    pub linear_layout_params_class: GlobalRef,
    pub frame_layout_params_class: GlobalRef,
    pub text_watcher_class: GlobalRef,
    pub perry_bridge_class: GlobalRef,

    // --- Constructor IDs ---
    pub text_view_init: JMethodID,
    pub button_init: JMethodID,
    pub linear_layout_init: JMethodID,
    pub scroll_view_init: JMethodID,
    pub edit_text_init: JMethodID,
    pub switch_init: JMethodID,
    pub seek_bar_init: JMethodID,
    pub space_init: JMethodID,
    pub view_init: JMethodID,
    pub popup_menu_init: JMethodID,
    pub linear_layout_params_init: JMethodID,
    pub frame_layout_params_init: JMethodID,

    // --- TextView methods ---
    pub text_view_set_text: JMethodID,
    pub text_view_set_text_color: JMethodID,
    pub text_view_set_text_size: JMethodID,
    pub text_view_set_typeface: JMethodID,
    pub text_view_set_text_is_selectable: JMethodID,
    pub text_view_get_text: JMethodID,

    // --- Button methods ---
    pub button_set_text: JMethodID,

    // --- View methods ---
    pub view_set_visibility: JMethodID,
    pub view_set_background_color: JMethodID,
    pub view_set_layout_params: JMethodID,
    pub view_set_padding: JMethodID,
    pub view_set_on_long_click_listener: JMethodID,
    pub view_request_focus: JMethodID,

    // --- ViewGroup methods ---
    pub view_group_add_view: JMethodID,
    pub view_group_add_view_at: JMethodID,
    pub view_group_remove_all_views: JMethodID,
    pub view_group_get_child_count: JMethodID,

    // --- LinearLayout methods ---
    pub linear_layout_set_orientation: JMethodID,

    // --- ScrollView methods ---
    pub scroll_view_add_view: JMethodID,
    pub scroll_view_scroll_to: JMethodID,
    pub scroll_view_get_scroll_y: JMethodID,
    pub scroll_view_smooth_scroll_to: JMethodID,

    // --- EditText methods ---
    pub edit_text_set_hint: JMethodID,
    pub edit_text_set_text: JMethodID,

    // --- SeekBar methods ---
    pub seek_bar_set_max: JMethodID,
    pub seek_bar_set_progress: JMethodID,
    pub seek_bar_get_progress: JMethodID,

    // --- Switch methods ---
    pub switch_set_checked: JMethodID,
    pub switch_is_checked: JMethodID,
    pub switch_set_text: JMethodID,

    // --- Color methods ---
    pub color_argb: JMethodID,

    // --- Typeface methods ---
    pub typeface_create: JMethodID,

    // --- PopupMenu methods ---
    pub popup_menu_get_menu: JMethodID,
    pub popup_menu_show: JMethodID,

    // --- PerryBridge methods ---
    pub perry_bridge_get_activity: JMethodID,
    pub perry_bridge_run_on_ui_thread: JMethodID,

    // --- dp conversion ---
    pub perry_bridge_dp_to_px: JMethodID,
}

thread_local! {
    static JNI_CACHE: RefCell<Option<JniCache>> = RefCell::new(None);
}

/// Initialize the global JavaVM reference. Called from JNI_OnLoad.
pub fn init_vm(vm: JavaVM) {
    let _ = JAVA_VM.set(vm);
}

/// Get a JNIEnv for the current thread.
pub fn get_env() -> JNIEnv<'static> {
    let vm = JAVA_VM.get().expect("JavaVM not initialized");
    // attach_current_thread_permanently is safe to call multiple times
    vm.attach_current_thread_permanently()
        .expect("Failed to attach JNI thread")
}

/// Get the JavaVM.
pub fn get_vm() -> &'static JavaVM {
    JAVA_VM.get().expect("JavaVM not initialized")
}

/// Initialize the JNI class/method cache for the current thread.
/// Must be called after JavaVM is set and from a thread with a valid JNIEnv.
pub fn init_cache(env: &mut JNIEnv) {
    JNI_CACHE.with(|cache| {
        if cache.borrow().is_some() {
            return;
        }

        let c = build_cache(env);
        *cache.borrow_mut() = Some(c);
    });
}

/// Access the cached JNI references. Lazily initializes the cache if not yet built
/// on the current thread (e.g., when called from the UI thread after nativeInit).
pub fn with_cache<F, R>(f: F) -> R
where
    F: FnOnce(&JniCache) -> R,
{
    JNI_CACHE.with(|cache| {
        // Lazy-init: build cache on first access per thread
        let needs_init = cache.borrow().is_none();
        if needs_init {
            let mut env = get_env();
            let c = build_cache(&mut env);
            *cache.borrow_mut() = Some(c);
        }
        let borrow = cache.borrow();
        let c = borrow.as_ref().unwrap();
        f(c)
    })
}

fn find_class(env: &mut JNIEnv, name: &str) -> GlobalRef {
    let class = env
        .find_class(name)
        .unwrap_or_else(|_| panic!("Failed to find class: {}", name));
    env.new_global_ref(class)
        .unwrap_or_else(|_| panic!("Failed to create global ref for: {}", name))
}

fn get_method(env: &mut JNIEnv, class: &GlobalRef, name: &str, sig: &str) -> JMethodID {
    let cls: &JClass = class.as_obj().into();
    env.get_method_id(cls, name, sig)
        .unwrap_or_else(|_| panic!("Failed to find method: {}::{} {}", "<class>", name, sig))
}

fn get_static_method(env: &mut JNIEnv, class: &GlobalRef, name: &str, sig: &str) -> JMethodID {
    let cls: &JClass = class.as_obj().into();
    // Static method IDs are the same type as instance method IDs in jni-rs
    let mid = env
        .get_static_method_id(cls, name, sig)
        .unwrap_or_else(|_| {
            panic!(
                "Failed to find static method: {}::{} {}",
                "<class>", name, sig
            )
        });
    // Cast — JStaticMethodID and JMethodID are both wrappers around jmethodID
    unsafe { std::mem::transmute(mid) }
}

fn build_cache(env: &mut JNIEnv) -> JniCache {
    // Find all classes
    let text_view_class = find_class(env, "android/widget/TextView");
    let button_class = find_class(env, "android/widget/Button");
    let linear_layout_class = find_class(env, "android/widget/LinearLayout");
    let scroll_view_class = find_class(env, "android/widget/ScrollView");
    let edit_text_class = find_class(env, "android/widget/EditText");
    let switch_class = find_class(env, "android/widget/Switch");
    let seek_bar_class = find_class(env, "android/widget/SeekBar");
    let space_class = find_class(env, "android/widget/Space");
    let view_class = find_class(env, "android/view/View");
    let color_class = find_class(env, "android/graphics/Color");
    let typeface_class = find_class(env, "android/graphics/Typeface");
    let popup_menu_class = find_class(env, "android/widget/PopupMenu");
    let clipboard_manager_class = find_class(env, "android/content/ClipboardManager");
    let context_class = find_class(env, "android/content/Context");
    let frame_layout_class = find_class(env, "android/widget/FrameLayout");
    let view_group_class = find_class(env, "android/view/ViewGroup");
    let view_group_layout_params_class = find_class(env, "android/view/ViewGroup$LayoutParams");
    let linear_layout_params_class = find_class(env, "android/widget/LinearLayout$LayoutParams");
    let frame_layout_params_class = find_class(env, "android/widget/FrameLayout$LayoutParams");
    let text_watcher_class = find_class(env, "android/text/TextWatcher");
    let perry_bridge_class = find_class(env, "com/perry/app/PerryBridge");

    // Constructor IDs — all View subclass constructors take (Context)
    let text_view_init = get_method(
        env,
        &text_view_class,
        "<init>",
        "(Landroid/content/Context;)V",
    );
    let button_init = get_method(env, &button_class, "<init>", "(Landroid/content/Context;)V");
    let linear_layout_init = get_method(
        env,
        &linear_layout_class,
        "<init>",
        "(Landroid/content/Context;)V",
    );
    let scroll_view_init = get_method(
        env,
        &scroll_view_class,
        "<init>",
        "(Landroid/content/Context;)V",
    );
    let edit_text_init = get_method(
        env,
        &edit_text_class,
        "<init>",
        "(Landroid/content/Context;)V",
    );
    let switch_init = get_method(env, &switch_class, "<init>", "(Landroid/content/Context;)V");
    let seek_bar_init = get_method(
        env,
        &seek_bar_class,
        "<init>",
        "(Landroid/content/Context;)V",
    );
    let space_init = get_method(env, &space_class, "<init>", "(Landroid/content/Context;)V");
    let view_init = get_method(env, &view_class, "<init>", "(Landroid/content/Context;)V");
    let popup_menu_init = get_method(
        env,
        &popup_menu_class,
        "<init>",
        "(Landroid/content/Context;Landroid/view/View;)V",
    );
    let linear_layout_params_init = get_method(env, &linear_layout_params_class, "<init>", "(II)V");
    let frame_layout_params_init = get_method(env, &frame_layout_params_class, "<init>", "(II)V");

    // TextView methods
    let text_view_set_text = get_method(
        env,
        &text_view_class,
        "setText",
        "(Ljava/lang/CharSequence;)V",
    );
    let text_view_set_text_color = get_method(env, &text_view_class, "setTextColor", "(I)V");
    let text_view_set_text_size = get_method(env, &text_view_class, "setTextSize", "(IF)V");
    let text_view_set_typeface = get_method(
        env,
        &text_view_class,
        "setTypeface",
        "(Landroid/graphics/Typeface;I)V",
    );
    let text_view_set_text_is_selectable =
        get_method(env, &text_view_class, "setTextIsSelectable", "(Z)V");
    let text_view_get_text = get_method(
        env,
        &text_view_class,
        "getText",
        "()Ljava/lang/CharSequence;",
    );

    // Button methods (inherits from TextView)
    let button_set_text = get_method(env, &button_class, "setText", "(Ljava/lang/CharSequence;)V");

    // View methods
    let view_set_visibility = get_method(env, &view_class, "setVisibility", "(I)V");
    let view_set_background_color = get_method(env, &view_class, "setBackgroundColor", "(I)V");
    let view_set_layout_params = get_method(
        env,
        &view_class,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
    );
    let view_set_padding = get_method(env, &view_class, "setPadding", "(IIII)V");
    let view_set_on_long_click_listener = get_method(
        env,
        &view_class,
        "setOnLongClickListener",
        "(Landroid/view/View$OnLongClickListener;)V",
    );
    let view_request_focus = get_method(env, &view_class, "requestFocus", "()Z");

    // ViewGroup methods
    let view_group_add_view =
        get_method(env, &view_group_class, "addView", "(Landroid/view/View;)V");
    let view_group_add_view_at =
        get_method(env, &view_group_class, "addView", "(Landroid/view/View;I)V");
    let view_group_remove_all_views = get_method(env, &view_group_class, "removeAllViews", "()V");
    let view_group_get_child_count = get_method(env, &view_group_class, "getChildCount", "()I");

    // LinearLayout methods
    let linear_layout_set_orientation =
        get_method(env, &linear_layout_class, "setOrientation", "(I)V");

    // ScrollView methods
    let scroll_view_add_view =
        get_method(env, &scroll_view_class, "addView", "(Landroid/view/View;)V");
    let scroll_view_scroll_to = get_method(env, &scroll_view_class, "scrollTo", "(II)V");
    let scroll_view_get_scroll_y = get_method(env, &scroll_view_class, "getScrollY", "()I");
    let scroll_view_smooth_scroll_to =
        get_method(env, &scroll_view_class, "smoothScrollTo", "(II)V");

    // EditText methods
    let edit_text_set_hint = get_method(
        env,
        &edit_text_class,
        "setHint",
        "(Ljava/lang/CharSequence;)V",
    );
    let edit_text_set_text = get_method(
        env,
        &edit_text_class,
        "setText",
        "(Ljava/lang/CharSequence;)V",
    );

    // SeekBar methods
    let seek_bar_set_max = get_method(env, &seek_bar_class, "setMax", "(I)V");
    let seek_bar_set_progress = get_method(env, &seek_bar_class, "setProgress", "(I)V");
    let seek_bar_get_progress = get_method(env, &seek_bar_class, "getProgress", "()I");

    // Switch methods
    let switch_set_checked = get_method(env, &switch_class, "setChecked", "(Z)V");
    let switch_is_checked = get_method(env, &switch_class, "isChecked", "()Z");
    let switch_set_text = get_method(env, &switch_class, "setText", "(Ljava/lang/CharSequence;)V");

    // Color static methods
    let color_argb = get_static_method(env, &color_class, "argb", "(IIII)I");

    // Typeface static methods
    let typeface_create = get_static_method(
        env,
        &typeface_class,
        "create",
        "(Ljava/lang/String;I)Landroid/graphics/Typeface;",
    );

    // PopupMenu methods
    let popup_menu_get_menu =
        get_method(env, &popup_menu_class, "getMenu", "()Landroid/view/Menu;");
    let popup_menu_show = get_method(env, &popup_menu_class, "show", "()V");

    // PerryBridge static methods
    let perry_bridge_get_activity = get_static_method(
        env,
        &perry_bridge_class,
        "getActivity",
        "()Landroid/app/Activity;",
    );
    let perry_bridge_run_on_ui_thread =
        get_static_method(env, &perry_bridge_class, "runOnUiThreadBlocking", "(J)V");
    let perry_bridge_dp_to_px = get_static_method(env, &perry_bridge_class, "dpToPx", "(F)I");

    JniCache {
        text_view_class,
        button_class,
        linear_layout_class,
        scroll_view_class,
        edit_text_class,
        switch_class,
        seek_bar_class,
        space_class,
        view_class,
        color_class,
        typeface_class,
        popup_menu_class,
        clipboard_manager_class,
        context_class,
        frame_layout_class,
        view_group_class,
        view_group_layout_params_class,
        linear_layout_params_class,
        frame_layout_params_class,
        text_watcher_class,
        perry_bridge_class,

        text_view_init,
        button_init,
        linear_layout_init,
        scroll_view_init,
        edit_text_init,
        switch_init,
        seek_bar_init,
        space_init,
        view_init,
        popup_menu_init,
        linear_layout_params_init,
        frame_layout_params_init,

        text_view_set_text,
        text_view_set_text_color,
        text_view_set_text_size,
        text_view_set_typeface,
        text_view_set_text_is_selectable,
        text_view_get_text,
        button_set_text,
        view_set_visibility,
        view_set_background_color,
        view_set_layout_params,
        view_set_padding,
        view_set_on_long_click_listener,
        view_request_focus,
        view_group_add_view,
        view_group_add_view_at,
        view_group_remove_all_views,
        view_group_get_child_count,
        linear_layout_set_orientation,
        scroll_view_add_view,
        scroll_view_scroll_to,
        scroll_view_get_scroll_y,
        scroll_view_smooth_scroll_to,
        edit_text_set_hint,
        edit_text_set_text,
        seek_bar_set_max,
        seek_bar_set_progress,
        seek_bar_get_progress,
        switch_set_checked,
        switch_is_checked,
        switch_set_text,
        color_argb,
        typeface_create,
        popup_menu_get_menu,
        popup_menu_show,
        perry_bridge_get_activity,
        perry_bridge_run_on_ui_thread,
        perry_bridge_dp_to_px,
    }
}
