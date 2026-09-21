//! The JS-visible `perry/tui` handle objects (#340/#341).
//!
//! Everything `perry/tui` hands back to TypeScript — a widget from `Text` /
//! `Box` / `Table` / …, a `state(0)` container, a `useRef` box, and the
//! `useApp` / `useStdout` / `useFocusManager` singletons — used to be a small
//! registry integer NaN-boxed with `POINTER_TAG`. Six independent id spaces
//! shared that one encoding and every one of them counts from a small
//! constant, so the values COLLIDED:
//!
//! ```text
//! useApp()          -> 1     Text("hi")       -> 1    useRef(x) (first) -> 1
//! useStdout()       -> 2     Box()            -> 2    useRef(y) (second)-> 2
//! useFocusManager() -> 3     Spacer()         -> 3
//! state(0) (first)  -> 0     <- POINTER_TAG | 0: a null pointer wearing the
//!                               pointer tag
//! ```
//!
//! So `useApp() === Text("hi")` was `true`, a `Map` keyed on two different
//! handles collapsed to one entry, and the first `state(0)` of a program was
//! literally a tagged null. None of that is reachable through a type error —
//! the values are indistinguishable at run time, because the encoding carries
//! no provenance.
//!
//! A tui handle is now an ORDINARY object: `GC_TYPE_OBJECT` with a real
//! ShapeId, a per-kind class id in the web-builtin block and (for the kinds
//! that have a method surface) a per-kind prototype. The registry id rides in
//! `ObjectMeta.native_state`, so the id stays the module's internal currency —
//! the widget tree, the layout pass, the paint pass and the hook slots all
//! still speak ids — and only the value that crosses into JS changes.
//!
//! The boundary is exactly the `#[no_mangle]` FFI surface: a producer wraps an
//! id on its way out, a consumer resolves an object back to an id on the way
//! in, and nothing between them changes. A raw argument is not a GC root, so
//! every consumer resolves at entry, before anything that can allocate.
//!
//! State word: bit 0 present, bits 8.. the registry id. The KIND is not in the
//! word — it is the class id on the object header, which is also what brands a
//! prototype method against a foreign receiver.

use std::sync::atomic::{AtomicI64, Ordering};

/// Class ids in the web-builtin block (`0xFFFF_24xx`), allocated by
/// [`crate::native_class_ids`]. `0x2401..=0x2406` are
/// AbortController/AbortSignal/Event/CustomEvent/DOMException/EventTarget,
/// `0x2407/8` TextEncoder/TextDecoder, `0x2409/A` Timeout/Immediate.
pub(crate) const WIDGET_CLASS_ID: u32 = crate::native_class_ids::TUI_WIDGET;
pub(crate) const STATE_CLASS_ID: u32 = crate::native_class_ids::TUI_STATE;
pub(crate) const REF_BOX_CLASS_ID: u32 = crate::native_class_ids::TUI_REF_BOX;
pub(crate) const APP_CLASS_ID: u32 = crate::native_class_ids::TUI_APP;
pub(crate) const STDOUT_CLASS_ID: u32 = crate::native_class_ids::TUI_STDOUT;
pub(crate) const FOCUS_MANAGER_CLASS_ID: u32 = crate::native_class_ids::TUI_FOCUS_MANAGER;

const TUI_STATE_PRESENT: u64 = 1;
const TUI_STATE_ID_SHIFT: u32 = 8;

/// The JS-visible kinds of `perry/tui` handle. One class id each, so a
/// prototype method can refuse a receiver from another kind instead of
/// reading its id as if it were one of its own — the six id spaces overlap,
/// so without the brand `state.get.call(someWidget)` would read a real state
/// slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TuiKind {
    /// `Text` / `Box` / `Spacer` / … — a node in the widget tree.
    Widget,
    /// `state(initial)` — a reactive slot with `.get()` / `.set(v)`.
    State,
    /// `useRef(initial)` — a hook slot with `.get()` / `.set(v)`.
    RefBox,
    /// `useApp()` — process-wide singleton.
    App,
    /// `useStdout()` — process-wide singleton.
    Stdout,
    /// `useFocusManager()` — process-wide singleton.
    FocusManager,
}

impl TuiKind {
    pub(crate) fn class_id(self) -> u32 {
        match self {
            TuiKind::Widget => WIDGET_CLASS_ID,
            TuiKind::State => STATE_CLASS_ID,
            TuiKind::RefBox => REF_BOX_CLASS_ID,
            TuiKind::App => APP_CLASS_ID,
            TuiKind::Stdout => STDOUT_CLASS_ID,
            TuiKind::FocusManager => FOCUS_MANAGER_CLASS_ID,
        }
    }

    fn from_class_id(class_id: u32) -> Option<Self> {
        Some(match class_id {
            WIDGET_CLASS_ID => TuiKind::Widget,
            STATE_CLASS_ID => TuiKind::State,
            REF_BOX_CLASS_ID => TuiKind::RefBox,
            APP_CLASS_ID => TuiKind::App,
            STDOUT_CLASS_ID => TuiKind::Stdout,
            FOCUS_MANAGER_CLASS_ID => TuiKind::FocusManager,
            _ => return None,
        })
    }
}

crate::perry_thread_local! {
    static STATE_PROTOTYPE_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static REF_BOX_PROTOTYPE_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static APP_PROTOTYPE_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static STDOUT_PROTOTYPE_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static FOCUS_MANAGER_PROTOTYPE_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static APP_SINGLETON_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static STDOUT_SINGLETON_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static FOCUS_MANAGER_SINGLETON_SLOT: AtomicI64 = const { AtomicI64::new(0) };
}

/// Per-kind prototype singletons, one per realm. Every handle of that kind
/// points its `[[Prototype]]` here, so they must outlive every handle — the
/// same rooting contract as the `%IteratorPrototype%` tower and the
/// `Timeout`/`Immediate` prototypes, and scanned from the same place
/// (`object::scan_object_cache_roots_mut`).
pub(crate) static STATE_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&STATE_PROTOTYPE_SLOT);
pub(crate) static REF_BOX_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&REF_BOX_PROTOTYPE_SLOT);
pub(crate) static APP_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&APP_PROTOTYPE_SLOT);
pub(crate) static STDOUT_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&STDOUT_PROTOTYPE_SLOT);
pub(crate) static FOCUS_MANAGER_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&FOCUS_MANAGER_PROTOTYPE_SLOT);

/// The three singleton handles. `useApp()` returned the same id on every call
/// so that reference semantics stayed stable across renders (ink's `useApp()`
/// does the same); with objects, "the same id" has to become "the same
/// object" or `useApp() === useApp()` would break. This is the resource ->
/// object mapping at singleton scale.
pub(crate) static APP_SINGLETON_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&APP_SINGLETON_SLOT);
pub(crate) static STDOUT_SINGLETON_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&STDOUT_SINGLETON_SLOT);
pub(crate) static FOCUS_MANAGER_SINGLETON_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&FOCUS_MANAGER_SINGLETON_SLOT);

/// GC roots for the prototypes and the three singletons. Called from
/// `object::scan_object_cache_roots_mut`, beside the timer prototypes.
pub(crate) fn scan_tui_handle_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    for slot in [
        &STATE_PROTOTYPE_PTR,
        &REF_BOX_PROTOTYPE_PTR,
        &APP_PROTOTYPE_PTR,
        &STDOUT_PROTOTYPE_PTR,
        &FOCUS_MANAGER_PROTOTYPE_PTR,
        &APP_SINGLETON_PTR,
        &STDOUT_SINGLETON_PTR,
        &FOCUS_MANAGER_SINGLETON_PTR,
    ] {
        slot.with_slot(|slot| {
            visitor.visit_atomic_i64_slot(slot, Ordering::Acquire, Ordering::Release);
        });
    }
}

fn state_word(id: i64) -> u64 {
    TUI_STATE_PRESENT | ((id as u64) << TUI_STATE_ID_SHIFT)
}

/// `(kind, id)` for a tui handle reached by its UNBOXED payload, or `None` for
/// anything else. `raw` is what codegen passes a native: the receiver and every
/// `NA_PTR` argument arrive already stripped of the NaN-box tag.
pub(crate) fn tui_handle_parts_raw(raw: i64) -> Option<(TuiKind, i64)> {
    if raw <= 0 {
        return None;
    }
    let addr = raw as usize;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = addr as *mut crate::object::ObjectHeader;
    unsafe {
        let kind = TuiKind::from_class_id((*obj).class_id)?;
        let meta = (*obj).meta;
        if meta.is_null() {
            return None;
        }
        let word = (*meta).native_state;
        if word & TUI_STATE_PRESENT == 0 {
            return None;
        }
        Some((kind, (word >> TUI_STATE_ID_SHIFT) as i64))
    }
}

/// The registry id behind a tui handle of the expected kind, or `None`.
///
/// The kind check is load-bearing rather than defensive: the six id spaces
/// overlap (widget 1, app 1 and the first `useRef` are all id 1), so a handle
/// of the wrong kind would resolve to a real, live entry of this one.
pub(crate) fn tui_handle_id(raw: i64, kind: TuiKind) -> Option<i64> {
    match tui_handle_parts_raw(raw) {
        Some((k, id)) if k == kind => Some(id),
        _ => None,
    }
}

/// The widget id behind a NaN-boxed JS value. Used where a widget handle
/// arrives as a boxed `f64` rather than as a native argument: elements of the
/// children array, and the value a `run()` component returns.
pub(crate) fn tui_widget_id_from_bits(bits: u64) -> i64 {
    let raw = (bits & crate::value::POINTER_MASK) as i64;
    tui_handle_id(raw, TuiKind::Widget).unwrap_or(0)
}

/// `(kind, id)` for a NaN-boxed JS value. The prototype thunks resolve their
/// receiver through this, since `js_implicit_this_get` hands back a boxed
/// value.
fn tui_handle_parts_value(value: f64) -> Option<(TuiKind, i64)> {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    tui_handle_parts_raw((bits & crate::value::POINTER_MASK) as i64)
}

/// The receiver of a prototype method, as an id of the expected kind.
///
/// `perry/tui` is not a WebIDL surface and has no node equivalent to copy a
/// brand-check policy from, so a foreign receiver is answered leniently with
/// `undefined` rather than thrown at — the same choice node makes for the
/// timer methods, and the one that cannot turn a working program into a
/// throwing one. It is never read as an id of this kind: that is what the
/// class-id brand prevents.
fn receiver_id(kind: TuiKind) -> Option<i64> {
    let this = crate::object::js_implicit_this_get();
    match tui_handle_parts_value(this) {
        Some((k, id)) if k == kind => Some(id),
        _ => None,
    }
}

const UNDEFINED: u64 = crate::value::TAG_UNDEFINED;

extern "C" fn state_proto_get_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    match receiver_id(TuiKind::State) {
        Some(id) => super::state::state_get_by_id(id),
        None => f64::from_bits(UNDEFINED),
    }
}

extern "C" fn state_proto_set_thunk(
    _c: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    if let Some(id) = receiver_id(TuiKind::State) {
        super::state::state_set_by_id(id, value);
    }
    f64::from_bits(UNDEFINED)
}

extern "C" fn ref_proto_get_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    match receiver_id(TuiKind::RefBox) {
        Some(id) => super::hooks::ref_get_by_id(id),
        None => f64::from_bits(UNDEFINED),
    }
}

extern "C" fn ref_proto_set_thunk(_c: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    if let Some(id) = receiver_id(TuiKind::RefBox) {
        super::hooks::ref_set_by_id(id, value);
    }
    f64::from_bits(UNDEFINED)
}

extern "C" fn app_proto_exit_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    if receiver_id(TuiKind::App).is_some() {
        super::input::EXIT_FLAG.store(true, Ordering::Release);
    }
    f64::from_bits(UNDEFINED)
}

extern "C" fn app_proto_wait_until_exit_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    if receiver_id(TuiKind::App).is_some() {
        super::hooks::wait_until_exit_blocking();
    }
    f64::from_bits(UNDEFINED)
}

extern "C" fn stdout_proto_write_thunk(
    _c: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    if receiver_id(TuiKind::Stdout).is_some() {
        let s = crate::value::js_jsvalue_to_string_coerce(value);
        super::hooks::stdout_write_string_ptr(s);
    }
    f64::from_bits(UNDEFINED)
}

extern "C" fn stdout_proto_columns_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    match receiver_id(TuiKind::Stdout) {
        Some(_) => super::hooks::js_perry_tui_stdout_columns(0),
        None => f64::from_bits(UNDEFINED),
    }
}

extern "C" fn stdout_proto_rows_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    match receiver_id(TuiKind::Stdout) {
        Some(_) => super::hooks::js_perry_tui_stdout_rows(0),
        None => f64::from_bits(UNDEFINED),
    }
}

extern "C" fn focus_proto_next_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    if receiver_id(TuiKind::FocusManager).is_some() {
        super::hooks::js_perry_tui_focus_next();
    }
    f64::from_bits(UNDEFINED)
}

extern "C" fn focus_proto_previous_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    if receiver_id(TuiKind::FocusManager).is_some() {
        super::hooks::js_perry_tui_focus_previous();
    }
    f64::from_bits(UNDEFINED)
}

extern "C" fn focus_proto_focus_thunk(_c: *const crate::closure::ClosureHeader, id: f64) -> f64 {
    if receiver_id(TuiKind::FocusManager).is_some() {
        super::hooks::js_perry_tui_focus(id);
    }
    f64::from_bits(UNDEFINED)
}

/// Build a kind's prototype into its rooted slot. Idempotent; lazy, because a
/// program that never touches `perry/tui` must not pay for any of it.
///
/// `TuiKind::Widget` has no method surface — a widget is an opaque handle you
/// pass to `render` or to `Box` — so it links no prototype and inherits
/// `Object.prototype` like any other bare object.
fn build_prototype(kind: TuiKind) -> *mut crate::object::ObjectHeader {
    let Some(slot) = prototype_slot(kind) else {
        return std::ptr::null_mut();
    };
    let existing = slot.load(Ordering::Acquire);
    if existing != 0 {
        return existing as *mut crate::object::ObjectHeader;
    }
    // Raw locals stay stable across the allocating installs below, exactly as
    // the iterator tower and the timer prototypes do (#7251).
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = crate::object::js_object_alloc(0, 0);
    if proto.is_null() {
        return std::ptr::null_mut();
    }
    let methods: &[(&str, *const u8, u32)] = match kind {
        TuiKind::State => &[
            ("get", state_proto_get_thunk as *const u8, 0),
            ("set", state_proto_set_thunk as *const u8, 1),
        ],
        TuiKind::RefBox => &[
            ("get", ref_proto_get_thunk as *const u8, 0),
            ("set", ref_proto_set_thunk as *const u8, 1),
        ],
        TuiKind::App => &[
            ("exit", app_proto_exit_thunk as *const u8, 0),
            (
                "waitUntilExit",
                app_proto_wait_until_exit_thunk as *const u8,
                0,
            ),
        ],
        TuiKind::Stdout => &[
            ("write", stdout_proto_write_thunk as *const u8, 1),
            ("columns", stdout_proto_columns_thunk as *const u8, 0),
            ("rows", stdout_proto_rows_thunk as *const u8, 0),
        ],
        TuiKind::FocusManager => &[
            ("focusNext", focus_proto_next_thunk as *const u8, 0),
            ("focusPrevious", focus_proto_previous_thunk as *const u8, 0),
            ("focus", focus_proto_focus_thunk as *const u8, 1),
        ],
        TuiKind::Widget => &[],
    };
    for (name, ptr, arity) in methods {
        crate::object::install_proto_method(proto, name, *ptr, *arity);
    }
    slot.store(proto as i64, Ordering::Release);
    proto
}

fn prototype_slot(kind: TuiKind) -> Option<&'static crate::object::RealmAtomicI64> {
    Some(match kind {
        TuiKind::State => &STATE_PROTOTYPE_PTR,
        TuiKind::RefBox => &REF_BOX_PROTOTYPE_PTR,
        TuiKind::App => &APP_PROTOTYPE_PTR,
        TuiKind::Stdout => &STDOUT_PROTOTYPE_PTR,
        TuiKind::FocusManager => &FOCUS_MANAGER_PROTOTYPE_PTR,
        TuiKind::Widget => return None,
    })
}

/// Wrap a registry id in the JS-visible handle object. The id itself stays the
/// module's internal currency — the tree, the layout pass and the hook slots
/// all keep speaking ids — so this is called only at the `#[no_mangle]` FFI
/// boundary, on the way out.
pub(crate) fn tui_object(kind: TuiKind, id: i64) -> i64 {
    let obj = crate::object::js_object_alloc(kind.class_id(), 0);
    if obj.is_null() {
        return 0;
    }
    // Building the prototype allocates (lazily, on the first handle of a
    // program) and `GC_TYPE_OBJECT` is movable, so the instance is re-read
    // through its handle after each allocating step.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(obj);
    let proto = build_prototype(kind);
    if !proto.is_null() {
        handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
            crate::object::prototype_chain::object_link_class_default_prototype(
                obj as usize,
                crate::value::js_nanbox_pointer(proto as i64).to_bits(),
            );
        });
    }
    handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| unsafe {
        let meta = crate::object::object_meta_ensure(obj);
        // A handle whose state word never landed would resolve to `None` and
        // every method on it would answer `undefined` -- silently, and only in
        // a low-memory run. The meta has to exist for the handle to mean
        // anything.
        debug_assert!(!meta.is_null(), "a tui handle must carry its meta");
        if !meta.is_null() {
            (*meta).native_state = state_word(id);
        }
    });
    handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| obj as i64)
}

/// The process-wide singleton handle for `useApp` / `useStdout` /
/// `useFocusManager`, minted once per realm.
///
/// `useApp() === useApp()` is `true` today because both calls answer the same
/// id, and ink's `useApp()` is likewise stable across renders. Objects only
/// keep that property if the SAME object comes back, so the singleton lives in
/// a rooted slot rather than being re-minted per call.
pub(crate) fn tui_singleton(kind: TuiKind, id: i64) -> i64 {
    let slot = match kind {
        TuiKind::App => &APP_SINGLETON_PTR,
        TuiKind::Stdout => &STDOUT_SINGLETON_PTR,
        TuiKind::FocusManager => &FOCUS_MANAGER_SINGLETON_PTR,
        _ => return tui_object(kind, id),
    };
    let existing = slot.load(Ordering::Acquire);
    if existing != 0 {
        return existing;
    }
    let obj = tui_object(kind, id);
    if obj != 0 {
        slot.store(obj, Ordering::Release);
    }
    obj
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GATE B, and the invariant the whole migration is for: the value JS
    /// receives is a real heap object with the kind's class id, ABOVE the
    /// small-handle band, carrying zero own keys. The band assertion is what
    /// covers statically lowered reads -- `state.get()` lowers through a
    /// `class_filter` row and never reaches an instrumented funnel, so the
    /// receiver-repr ledger (gate A) cannot see it.
    #[test]
    fn a_tui_handle_is_an_ordinary_object_outside_the_handle_band() {
        let cases = [
            (super::super::ffi::js_perry_tui_box(), TuiKind::Widget),
            (
                super::super::state::js_perry_tui_state_alloc(0.0),
                TuiKind::State,
            ),
            (super::super::hooks::js_perry_tui_use_app(), TuiKind::App),
            (
                super::super::hooks::js_perry_tui_use_stdout(),
                TuiKind::Stdout,
            ),
            (
                super::super::hooks::js_perry_tui_use_focus_manager(),
                TuiKind::FocusManager,
            ),
        ];
        for (raw, kind) in cases {
            let addr = raw as usize;
            assert!(
                !crate::value::addr_class::is_handle_band(addr),
                "gate B: {kind:?} handed back a small band id ({addr:#x})"
            );
            let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }
                .expect("a tui handle carries a GcHeader");
            assert_eq!(header.obj_type, crate::gc::GC_TYPE_OBJECT);
            let obj = addr as *mut crate::object::ObjectHeader;
            assert_eq!(unsafe { (*obj).class_id }, kind.class_id());
            let keys = unsafe { crate::object::object_keys_array(obj) };
            let key_count = if keys.is_null() {
                0
            } else {
                unsafe { (*keys).length }
            };
            assert_eq!(key_count, 0, "a {kind:?} handle must have no own keys");
            assert!(
                tui_handle_id(raw, kind).is_some(),
                "a {kind:?} handle must resolve back to its id"
            );
        }
    }

    /// The six id spaces overlap, so the brand is what keeps them apart. Two
    /// handles of DIFFERENT kinds that carry the SAME id must not resolve
    /// through each other -- before this change they were literally the same
    /// value.
    #[test]
    fn a_handle_of_another_kind_never_resolves_as_this_one() {
        let widget = super::super::ffi::js_perry_tui_box();
        let app = super::super::hooks::js_perry_tui_use_app();
        assert_ne!(widget, app, "two kinds must be two objects");
        assert!(tui_handle_id(widget, TuiKind::App).is_none());
        assert!(tui_handle_id(app, TuiKind::Widget).is_none());
        // A plain object, a non-pointer value and 0 are all refused.
        let plain = crate::object::js_object_alloc(0, 0) as i64;
        assert!(tui_handle_parts_raw(plain).is_none());
        assert!(tui_handle_parts_raw(0).is_none());
        assert!(tui_handle_parts_raw(7).is_none());
    }

    /// `useApp()` / `useStdout()` / `useFocusManager()` are stable across
    /// calls -- ink's are, and they were trivially stable before this change
    /// because they were constants. A per-call mint would break
    /// `useApp() === useApp()` without breaking any method.
    #[test]
    fn the_singletons_are_one_object_per_realm() {
        assert_eq!(
            super::super::hooks::js_perry_tui_use_app(),
            super::super::hooks::js_perry_tui_use_app()
        );
        assert_eq!(
            super::super::hooks::js_perry_tui_use_stdout(),
            super::super::hooks::js_perry_tui_use_stdout()
        );
        assert_eq!(
            super::super::hooks::js_perry_tui_use_focus_manager(),
            super::super::hooks::js_perry_tui_use_focus_manager()
        );
        // ... and the three singletons are three DIFFERENT objects, where the
        // old encoding made them ids 1, 2 and 3 in one space shared with every
        // widget.
        let a = super::super::hooks::js_perry_tui_use_app();
        let s = super::super::hooks::js_perry_tui_use_stdout();
        let f = super::super::hooks::js_perry_tui_use_focus_manager();
        assert_ne!(a, s);
        assert_ne!(s, f);
        assert_ne!(a, f);
    }

    /// Two widgets are two objects. The old encoding made `Text("a")` and
    /// `Text("b")` ids 1 and 2 -- distinct -- but made `useApp()` and the
    /// first widget BOTH id 1, and this is the property that has to hold for
    /// `Map` / `Set` / `WeakMap` keys to work at all.
    #[test]
    fn distinct_widgets_are_distinct_objects() {
        let a = super::super::ffi::js_perry_tui_box();
        let b = super::super::ffi::js_perry_tui_box();
        assert_ne!(a, b);
        let ida = tui_handle_id(a, TuiKind::Widget).unwrap();
        let idb = tui_handle_id(b, TuiKind::Widget).unwrap();
        assert_ne!(ida, idb, "two widgets must be two tree entries");
    }

    /// The first `state(0)` of a program used to be `POINTER_TAG | 0` -- a
    /// null pointer wearing the pointer tag, which is the exact shape the
    /// invariant exists to forbid. Slot 0 is still a legal slot id; it is the
    /// `present` bit that distinguishes "slot 0" from "no state".
    #[test]
    fn state_slot_zero_is_a_real_object_not_a_tagged_null() {
        let first = super::super::state::js_perry_tui_state_alloc(0.0);
        assert_ne!(first, 0, "a state handle must never be a tagged null");
        let id = tui_handle_id(first, TuiKind::State).expect("state handle resolves");
        assert!(id >= 0);
    }
}
