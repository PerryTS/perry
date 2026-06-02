//! Continuous pointer events for perry/ui (issue #1868).
//!
//! Wires `onMouseDown`, `onMouseUp`, `onMouseMove` for any widget on
//! macOS, using a single lazily-installed `NSEvent` local monitor that
//! hit-tests against the registered widget set. The monitor returns the
//! event unconsumed, so normal click / drag handling in underlying
//! widgets continues to work alongside our callbacks.
//!
//! Why a global monitor instead of a per-widget overlay or NSView
//! subclass: Perry's widgets are stock `NSButton` / `NSTextField` / etc.
//! that we don't own and don't want to subclass. Overlays would have to
//! pierce hit-testing to forward clicks back to the underlying control,
//! and per-event responder-chain forwarding gets fiddly. A single local
//! monitor cleanly observes events and is the supported macOS API for
//! this exact case (NSEvent class reference).
//!
//! Callbacks receive a `PointerEvent { x, y, button, pointerType }`
//! object built by `js_pointer_event_new` (perry-runtime).
//! Coordinates are widget-local points with top-left origin (NSView's
//! bottom-left y is flipped here).

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGPoint, CGRect};
use std::cell::RefCell;
use std::collections::HashMap;

use crate::widgets::get_widget;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_pointer_event_new(x: f64, y: f64, button: u32, pointer_type: u32) -> f64;
}

const POINTER_TYPE_MOUSE: u32 = 0;

/// Boolean NaN-box payloads — perry-runtime/value/tags.rs holds these
/// crate-internal, so we mirror the bit patterns here for hover dispatch.
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

// ---------------------------------------------------------------------------
// NSEventType values we care about. From Apple's NSEvent.h.
// ---------------------------------------------------------------------------
const NS_EVENT_TYPE_LEFT_MOUSE_DOWN: u64 = 1;
const NS_EVENT_TYPE_LEFT_MOUSE_UP: u64 = 2;
const NS_EVENT_TYPE_RIGHT_MOUSE_DOWN: u64 = 3;
const NS_EVENT_TYPE_RIGHT_MOUSE_UP: u64 = 4;
const NS_EVENT_TYPE_MOUSE_MOVED: u64 = 5;
const NS_EVENT_TYPE_LEFT_MOUSE_DRAGGED: u64 = 6;
const NS_EVENT_TYPE_RIGHT_MOUSE_DRAGGED: u64 = 7;
const NS_EVENT_TYPE_OTHER_MOUSE_DOWN: u64 = 25;
const NS_EVENT_TYPE_OTHER_MOUSE_UP: u64 = 26;
const NS_EVENT_TYPE_OTHER_MOUSE_DRAGGED: u64 = 27;

/// NSEventMask = `1 << NSEventType`. We listen to every mouse event
/// type that can possibly trigger one of our three callbacks.
const NS_EVENT_MASK: u64 = (1 << NS_EVENT_TYPE_LEFT_MOUSE_DOWN)
    | (1 << NS_EVENT_TYPE_LEFT_MOUSE_UP)
    | (1 << NS_EVENT_TYPE_RIGHT_MOUSE_DOWN)
    | (1 << NS_EVENT_TYPE_RIGHT_MOUSE_UP)
    | (1 << NS_EVENT_TYPE_MOUSE_MOVED)
    | (1 << NS_EVENT_TYPE_LEFT_MOUSE_DRAGGED)
    | (1 << NS_EVENT_TYPE_RIGHT_MOUSE_DRAGGED)
    | (1 << NS_EVENT_TYPE_OTHER_MOUSE_DOWN)
    | (1 << NS_EVENT_TYPE_OTHER_MOUSE_UP)
    | (1 << NS_EVENT_TYPE_OTHER_MOUSE_DRAGGED);

thread_local! {
    static MOUSE_DOWN_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static MOUSE_UP_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static MOUSE_MOVE_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    /// Hover callbacks delivered with `(isHovering: boolean)` — issue
    /// #1868 reworked the API from enter-only to enter+leave through a
    /// single callback. Implemented by tracking per-widget hover state
    /// inside the same NSEvent monitor: a widget transitions in when a
    /// mouse-moved event lands inside its bounds and was previously out,
    /// and vice-versa.
    static HOVER_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    /// `true` if the cursor was inside the widget on the last move
    /// event we processed.
    static HOVER_STATE: RefCell<HashMap<i64, bool>> = RefCell::new(HashMap::new());
    /// Per-widget last (x,y) sent to the move callback within the current
    /// frame. Used to coalesce duplicate mouse-moved events: if AppKit
    /// emits 4 moves for the same window pos (resize + tracking-area
    /// updates can do this), we collapse them to one callback per frame.
    /// Cleared by the monitor block opportunistically.
    static MOVE_DEDUP: RefCell<HashMap<i64, (f64, f64)>> = RefCell::new(HashMap::new());
    static MONITOR_INSTALLED: RefCell<bool> = const { RefCell::new(false) };
    /// Window's `acceptsMouseMovedEvents` is required for AppKit to
    /// emit `NSEventTypeMouseMoved` at all. We flip it on for every
    /// window we observe lazily.
    static WINDOWS_PRIMED: RefCell<std::collections::HashSet<usize>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Install the single global NSEvent local monitor on first use. The
/// monitor block is leaked intentionally (lives for the app's lifetime)
/// — `addLocalMonitorForEventsMatchingMask:handler:` retains the block
/// internally too, but we forget our `RcBlock` to make the lifetime
/// clear at the call site.
fn install_monitor_once() {
    let already = MONITOR_INSTALLED.with(|m| *m.borrow());
    if already {
        return;
    }
    MONITOR_INSTALLED.with(|m| *m.borrow_mut() = true);

    use block2::RcBlock;

    let block = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
        if !event.is_null() {
            crate::catch_callback_panic(
                "pointer event monitor",
                std::panic::AssertUnwindSafe(|| unsafe {
                    handle_event(event);
                }),
            );
        }
        // Return the event so AppKit keeps dispatching it normally.
        event
    });

    unsafe {
        let ns_event_cls = class!(NSEvent);
        let _: *mut AnyObject = msg_send![
            ns_event_cls,
            addLocalMonitorForEventsMatchingMask: NS_EVENT_MASK,
            handler: &*block
        ];
    }
    std::mem::forget(block);
}

/// Ensure the window hosting `view` is set to deliver mouse-moved events.
/// AppKit gates `NSEventTypeMouseMoved` on `[NSWindow acceptsMouseMovedEvents]`
/// being true; otherwise our monitor never sees the event at all.
fn prime_window_for_view(view: &NSView) {
    unsafe {
        let window: *mut AnyObject = msg_send![view, window];
        if window.is_null() {
            // Will be primed lazily once the view enters a window —
            // the monitor still sees down/up events even without this.
            return;
        }
        let already = WINDOWS_PRIMED.with(|s| s.borrow().contains(&(window as usize)));
        if !already {
            let _: () = msg_send![window, setAcceptsMouseMovedEvents: true];
            WINDOWS_PRIMED.with(|s| {
                s.borrow_mut().insert(window as usize);
            });
        }
    }
}

/// Read the JS button index (0=Left, 1=Middle, 2=Right, 3=Back, 4=Forward)
/// from an NSEvent. macOS gives us `[NSEvent buttonNumber]` (0=left, 1=right,
/// 2=middle, 3+=other) — we remap to the web convention to match the
/// public API.
unsafe fn js_button_from_event(event: *mut AnyObject, event_type: u64) -> u32 {
    match event_type {
        NS_EVENT_TYPE_LEFT_MOUSE_DOWN
        | NS_EVENT_TYPE_LEFT_MOUSE_UP
        | NS_EVENT_TYPE_LEFT_MOUSE_DRAGGED => 0,
        NS_EVENT_TYPE_RIGHT_MOUSE_DOWN
        | NS_EVENT_TYPE_RIGHT_MOUSE_UP
        | NS_EVENT_TYPE_RIGHT_MOUSE_DRAGGED => 2,
        NS_EVENT_TYPE_OTHER_MOUSE_DOWN
        | NS_EVENT_TYPE_OTHER_MOUSE_UP
        | NS_EVENT_TYPE_OTHER_MOUSE_DRAGGED => {
            let n: i64 = msg_send![event, buttonNumber];
            // macOS "other" buttonNumber: 2=middle, 3=back, 4=forward, ...
            match n {
                2 => 1, // middle
                3 => 3, // back
                4 => 4, // forward
                _ => n.max(0) as u32,
            }
        }
        // mouseMoved with no buttons pressed: report 0 (web parity).
        _ => 0,
    }
}

/// Per-event dispatch entry point. Walks the registered-widget set,
/// hit-tests the event's window location against each widget's bounds,
/// and fires the matching callback(s).
unsafe fn handle_event(event: *mut AnyObject) {
    let event_type: u64 = msg_send![event, type];
    let window: *mut AnyObject = msg_send![event, window];
    if window.is_null() {
        return;
    }
    let win_loc: CGPoint = msg_send![event, locationInWindow];
    let button = js_button_from_event(event, event_type);

    // Pick which registry to dispatch to. Drag events are routed to
    // onMouseMove (drawing apps want a continuous stream during drag),
    // matching the issue's "coordinates as it moves within a widget"
    // language.
    let registry: fn() -> Vec<(i64, f64)> = match event_type {
        NS_EVENT_TYPE_LEFT_MOUSE_DOWN
        | NS_EVENT_TYPE_RIGHT_MOUSE_DOWN
        | NS_EVENT_TYPE_OTHER_MOUSE_DOWN => snapshot_down,
        NS_EVENT_TYPE_LEFT_MOUSE_UP
        | NS_EVENT_TYPE_RIGHT_MOUSE_UP
        | NS_EVENT_TYPE_OTHER_MOUSE_UP => snapshot_up,
        NS_EVENT_TYPE_MOUSE_MOVED
        | NS_EVENT_TYPE_LEFT_MOUSE_DRAGGED
        | NS_EVENT_TYPE_RIGHT_MOUSE_DRAGGED
        | NS_EVENT_TYPE_OTHER_MOUSE_DRAGGED => snapshot_move,
        _ => return,
    };

    let entries = registry();
    if entries.is_empty() {
        return;
    }

    let is_move = matches!(
        event_type,
        NS_EVENT_TYPE_MOUSE_MOVED
            | NS_EVENT_TYPE_LEFT_MOUSE_DRAGGED
            | NS_EVENT_TYPE_RIGHT_MOUSE_DRAGGED
            | NS_EVENT_TYPE_OTHER_MOUSE_DRAGGED
    );

    // On any move event, also drive `onHover(isHovering)` transitions
    // — we re-use the same hit-test the move callback runs. Walking
    // HOVER_CB once per move keeps the work O(n_hover_widgets), which
    // is typically a handful even in big UIs.
    if is_move {
        dispatch_hover_transitions(window, win_loc);
    }

    for (handle, closure_f64) in entries {
        let Some(view) = get_widget(handle) else {
            continue;
        };
        // Only fire for widgets that live in the event's window.
        let view_window: *mut AnyObject = msg_send![&*view, window];
        if view_window != window {
            continue;
        }
        // Convert window-coords → view-local (NSView's `convertPoint:fromView:`
        // with `fromView: nil` interprets the source as window coordinates).
        let local: CGPoint = msg_send![
            &*view,
            convertPoint: win_loc,
            fromView: std::ptr::null::<AnyObject>()
        ];
        let bounds: CGRect = msg_send![&*view, bounds];
        if local.x < bounds.origin.x
            || local.y < bounds.origin.y
            || local.x > bounds.origin.x + bounds.size.width
            || local.y > bounds.origin.y + bounds.size.height
        {
            continue;
        }
        // Flip y: NSView's origin is bottom-left; PointerEvent's is top-left.
        let local_x = local.x - bounds.origin.x;
        let local_y = bounds.size.height - (local.y - bounds.origin.y);

        // Coalesce move events that report the same (x, y) for the same
        // widget — AppKit can emit duplicates on tracking-area refresh.
        if is_move {
            let dedup_hit = MOVE_DEDUP.with(|m| {
                let mut b = m.borrow_mut();
                match b.get(&handle) {
                    Some(&(px, py)) if px == local_x && py == local_y => true,
                    _ => {
                        b.insert(handle, (local_x, local_y));
                        false
                    }
                }
            });
            if dedup_hit {
                continue;
            }
        }

        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        if closure_ptr == 0 {
            continue;
        }
        let pe = js_pointer_event_new(local_x, local_y, button, POINTER_TYPE_MOUSE);
        js_closure_call1(closure_ptr as *const u8, pe);
    }
}

/// Walk every hover-registered widget, hit-test the current event
/// location, and emit a `(isHovering)` callback whenever the widget's
/// containment state changed since the previous move. Tracking is
/// purely state-machine based — we do not install any per-view
/// NSTrackingArea, so it doesn't matter whether the widget is a
/// Perry-owned subclass or a stock NSButton/NSTextField.
unsafe fn dispatch_hover_transitions(event_window: *mut AnyObject, win_loc: CGPoint) {
    let entries: Vec<(i64, f64)> =
        HOVER_CB.with(|c| c.borrow().iter().map(|(k, v)| (*k, *v)).collect());
    for (handle, closure_f64) in entries {
        let Some(view) = get_widget(handle) else {
            continue;
        };
        let view_window: *mut AnyObject = msg_send![&*view, window];
        let inside = if view_window == event_window && !event_window.is_null() {
            let local: CGPoint = msg_send![
                &*view,
                convertPoint: win_loc,
                fromView: std::ptr::null::<AnyObject>()
            ];
            let bounds: CGRect = msg_send![&*view, bounds];
            local.x >= bounds.origin.x
                && local.y >= bounds.origin.y
                && local.x <= bounds.origin.x + bounds.size.width
                && local.y <= bounds.origin.y + bounds.size.height
        } else {
            false
        };
        let was_inside = HOVER_STATE.with(|s| s.borrow().get(&handle).copied().unwrap_or(false));
        if inside == was_inside {
            continue;
        }
        HOVER_STATE.with(|s| {
            s.borrow_mut().insert(handle, inside);
        });
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        if closure_ptr == 0 {
            continue;
        }
        let bool_bits = if inside { TAG_TRUE } else { TAG_FALSE };
        js_closure_call1(closure_ptr as *const u8, f64::from_bits(bool_bits));
    }
}

fn snapshot_down() -> Vec<(i64, f64)> {
    MOUSE_DOWN_CB.with(|c| c.borrow().iter().map(|(k, v)| (*k, *v)).collect())
}
fn snapshot_up() -> Vec<(i64, f64)> {
    MOUSE_UP_CB.with(|c| c.borrow().iter().map(|(k, v)| (*k, *v)).collect())
}
fn snapshot_move() -> Vec<(i64, f64)> {
    MOUSE_MOVE_CB.with(|c| c.borrow().iter().map(|(k, v)| (*k, *v)).collect())
}

// ---------------------------------------------------------------------------
// Public registration API. Called by FFI wrappers in lib_ffi/.
// ---------------------------------------------------------------------------

pub fn set_on_mouse_down(handle: i64, callback: f64) {
    MOUSE_DOWN_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    install_monitor_once();
    if let Some(view) = get_widget(handle) {
        prime_window_for_view(&view);
    }
}

pub fn set_on_mouse_up(handle: i64, callback: f64) {
    MOUSE_UP_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    install_monitor_once();
    if let Some(view) = get_widget(handle) {
        prime_window_for_view(&view);
    }
}

pub fn set_on_mouse_move(handle: i64, callback: f64) {
    MOUSE_MOVE_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    install_monitor_once();
    if let Some(view) = get_widget(handle) {
        prime_window_for_view(&view);
    }
}

/// Set the `(isHovering: boolean)`-style hover callback (issue #1868).
/// Replaces the prior enter-only `set_on_hover` in `widgets/mod.rs`, which
/// now delegates to this entry. The callback fires on both enter and
/// leave with the new state.
pub fn set_on_hover_v2(handle: i64, callback: f64) {
    HOVER_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    HOVER_STATE.with(|s| {
        s.borrow_mut().insert(handle, false);
    });
    install_monitor_once();
    if let Some(view) = get_widget(handle) {
        prime_window_for_view(&view);
    }
}
