//! Native compatibility facade for `@parcel/watcher` 2.5.x.
//!
//! The npm package's JavaScript `wrapper.js` normalizes user-facing ignore
//! globs and paths before calling this binding object. This crate implements
//! that binding directly with `notify`, so platform packages whose `main` is a
//! `.node` addon remain usable in a single static Perry binary.

use fancy_regex::Regex;
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use perry_ffi::{
    alloc_string, build_object_shape, drop_handle, error_value_with_code,
    gc_register_mutable_root_scanner_named, get_handle, iter_handles_of_mut, js_array_alloc,
    js_array_get, js_array_length, js_array_push, js_object_alloc_with_shape, js_object_set_field,
    notify_main_thread, object_field_by_name, read_string, register_aux_event_pump,
    register_handle, ErrorKind, GcRootVisitor, Handle, JsClosure, JsPromise, JsString, JsValue,
    Promise, RawClosureHeader, StringHeader, TransientRootScope,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Once};
use std::time::{Duration, Instant, UNIX_EPOCH};

extern "C" {
    fn js_get_string_pointer_unified(value: f64) -> i64;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WatchOptionsKey {
    backend: Option<String>,
    ignore_paths: Vec<PathBuf>,
    ignore_globs: Vec<String>,
}

#[derive(Clone)]
struct IgnoreMatcher {
    paths: Vec<PathBuf>,
    globs: Vec<Regex>,
}

impl IgnoreMatcher {
    fn compile(key: &WatchOptionsKey) -> Result<Self, String> {
        let mut globs = Vec::with_capacity(key.ignore_globs.len());
        for source in &key.ignore_globs {
            globs.push(
                Regex::new(source)
                    .map_err(|error| format!("Invalid @parcel/watcher ignore regex: {error}"))?,
            );
        }
        Ok(Self {
            paths: key.ignore_paths.clone(),
            globs,
        })
    }

    fn ignores(&self, root: &Path, path: &Path) -> bool {
        if self.paths.iter().any(|ignored| path.starts_with(ignored)) {
            return true;
        }
        let relative = path.strip_prefix(root).unwrap_or(path);
        let relative = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(value) => Some(value.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        self.globs
            .iter()
            .any(|regex| regex.is_match(&relative).unwrap_or(false))
    }
}

#[derive(Clone, Debug)]
enum Change {
    Create(PathBuf),
    Update(PathBuf),
    Delete(PathBuf),
    Rescan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoalescedKind {
    Create,
    Update,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParcelEvent {
    path: PathBuf,
    kind: CoalescedKind,
}

#[derive(Clone, Debug)]
enum Pending {
    Changes(u64, Vec<Change>),
    Error(u64, String),
}

#[derive(Clone, Debug)]
struct PendingItem {
    queued_at: Instant,
    event: Pending,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SnapshotEntry {
    is_dir: bool,
    len: u64,
    modified_ns: u64,
}

type Snapshot = BTreeMap<String, SnapshotEntry>;

struct Subscription {
    // Dropping notify's watcher stops its OS resource and joins its worker.
    _watcher: RecommendedWatcher,
    root: PathBuf,
    callback_handle: Handle,
    options: WatchOptionsKey,
    matcher: IgnoreMatcher,
    active: Arc<AtomicBool>,
    snapshot: Snapshot,
}

/// A stable registry identity for a moving-GC closure.
///
/// `unsubscribe` receives the closure's current address, which may differ from
/// the address passed to `subscribe` after a copying collection. The handle is
/// stable while this slot is rewritten by the registered root scanner.
struct CallbackRoot {
    callback: i64,
}

static SUBSCRIPTIONS: LazyLock<Mutex<HashMap<u64, Subscription>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PENDING: LazyLock<Mutex<Vec<PendingItem>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static NATIVE_EVENT_COUNT: AtomicU64 = AtomicU64::new(0);
static BATCH_WAKE_SCHEDULED: AtomicBool = AtomicBool::new(false);
static REGISTERED: Once = Once::new();
const BATCH_WINDOW: Duration = Duration::from_millis(10);

fn ensure_registered() {
    REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-parcel-watcher", scan_subscription_roots);
        register_aux_event_pump(
            js_parcel_watcher_process_pending,
            js_parcel_watcher_has_active,
        );
    });
}

fn scan_subscription_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<CallbackRoot, _>(|root| {
        visitor.visit_i64_slot(&mut root.callback);
    });
}

fn schedule_batch_wake() {
    if BATCH_WAKE_SCHEDULED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(BATCH_WINDOW);
        BATCH_WAKE_SCHEDULED.store(false, Ordering::Release);
        notify_main_thread();
    });
}

fn queue_pending(event: Pending) {
    PENDING.lock().unwrap().push(PendingItem {
        queued_at: Instant::now(),
        event,
    });
    schedule_batch_wake();
}

fn normalize_root(path: PathBuf) -> PathBuf {
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    fs::canonicalize(&absolute).unwrap_or(absolute)
}

unsafe fn read_ptr_string(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    read_string(JsString::from_raw(ptr as *mut StringHeader)).map(str::to_owned)
}

unsafe fn string_from_value(value: JsValue) -> Option<String> {
    if !value.is_any_string() {
        return None;
    }
    let ptr = js_get_string_pointer_unified(f64::from_bits(value.bits())) as *const StringHeader;
    read_ptr_string(ptr)
}

unsafe fn string_array_field(options: JsValue, name: &str) -> Vec<String> {
    let value = object_field_by_name(options, name);
    if !value.is_pointer() {
        return Vec::new();
    }
    let scope = TransientRootScope::enter();
    let array = scope.root_nanbox(f64::from_bits(value.bits()));
    let len = js_array_length(JsValue::from_bits(array.get().to_bits()).as_pointer());
    (0..len)
        .filter_map(|index| {
            let current = JsValue::from_bits(array.get().to_bits()).as_pointer();
            string_from_value(js_array_get(current, index))
        })
        .collect()
}

unsafe fn parse_options(options_bits: f64) -> WatchOptionsKey {
    let options = JsValue::from_bits(options_bits.to_bits());
    if !options.is_pointer_or_raw() {
        return WatchOptionsKey::default();
    }
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options_bits);
    let current = || JsValue::from_bits(options.get().to_bits());
    let backend = string_from_value(object_field_by_name(current(), "backend"));
    // Unknown and platform-unavailable backends deliberately fall through to
    // notify's recommended watcher, matching Parcel's documented fallback.
    let backend = backend.filter(|value| {
        matches!(
            value.as_str(),
            "fs-events" | "watchman" | "inotify" | "kqueue" | "windows" | "brute-force"
        )
    });
    WatchOptionsKey {
        backend,
        ignore_paths: string_array_field(current(), "ignorePaths")
            .into_iter()
            .map(|path| normalize_root(PathBuf::from(path)))
            .collect(),
        ignore_globs: string_array_field(current(), "ignoreGlobs"),
    }
}

fn changes_from_notify(event: Event) -> Vec<Change> {
    if event.need_rescan() {
        return vec![Change::Rescan];
    }
    match event.kind {
        EventKind::Create(_) => event.paths.into_iter().map(Change::Create).collect(),
        EventKind::Remove(_) => event.paths.into_iter().map(Change::Delete).collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() >= 2 => {
            vec![
                Change::Delete(event.paths[0].clone()),
                Change::Create(event.paths[1].clone()),
            ]
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            event.paths.into_iter().map(Change::Delete).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            event.paths.into_iter().map(Change::Create).collect()
        }
        // FSEvents and several fallback backends can report a rename without
        // paired paths. A fresh snapshot is the only reliable way to recover
        // Parcel's required `delete old` + `create new` semantics (and the
        // descendants of renamed directories).
        EventKind::Modify(ModifyKind::Name(_)) => vec![Change::Rescan],
        EventKind::Modify(_) | EventKind::Any | EventKind::Other => {
            event.paths.into_iter().map(Change::Update).collect()
        }
        EventKind::Access(_) => Vec::new(),
    }
}

fn snapshot_entry(path: &Path) -> Option<SnapshotEntry> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or(0);
    Some(SnapshotEntry {
        is_dir: metadata.is_dir(),
        len: metadata.len(),
        modified_ns,
    })
}

fn snapshot_tree(root: &Path, matcher: &IgnoreMatcher) -> Result<Snapshot, String> {
    fn visit(
        root: &Path,
        current: &Path,
        matcher: &IgnoreMatcher,
        out: &mut Snapshot,
    ) -> Result<(), String> {
        let entries = fs::read_dir(current)
            .map_err(|error| format!("Unable to read {}: {error}", current.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if matcher.ignores(root, &path) {
                continue;
            }
            let Some(metadata) = snapshot_entry(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let is_dir = metadata.is_dir;
            out.insert(relative, metadata);
            if is_dir {
                visit(root, &path, matcher, out)?;
            }
        }
        Ok(())
    }

    let mut snapshot = Snapshot::new();
    visit(root, root, matcher, &mut snapshot)?;
    Ok(snapshot)
}

fn diff_snapshots(root: &Path, old: &Snapshot, new: &Snapshot) -> Vec<ParcelEvent> {
    let mut events = Vec::new();
    // Parcel specifies rename as delete-old followed by create-new. Snapshot
    // recovery cannot know which paths were paired, so emit every deletion
    // before creations while retaining deterministic path order within each
    // group.
    for path in old.keys() {
        if !new.contains_key(path) {
            events.push(ParcelEvent {
                path: root.join(path),
                kind: CoalescedKind::Delete,
            });
        }
    }
    for (path, entry) in new {
        let kind = match old.get(path) {
            None => Some(CoalescedKind::Create),
            Some(previous) if previous != entry => Some(CoalescedKind::Update),
            _ => None,
        };
        if let Some(kind) = kind {
            events.push(ParcelEvent {
                path: root.join(path),
                kind,
            });
        }
    }
    events
}

fn snapshot_key(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

fn coalesce(
    root: &Path,
    matcher: &IgnoreMatcher,
    snapshot: &Snapshot,
    changes: &[Change],
) -> Vec<ParcelEvent> {
    let mut touched = BTreeSet::<PathBuf>::new();
    for change in changes {
        let path = match change {
            Change::Create(path) | Change::Update(path) | Change::Delete(path) => path,
            Change::Rescan => continue,
        };
        let path = normalize_root(path.clone());
        if matcher.ignores(root, &path) {
            continue;
        }
        touched.insert(path);
    }

    // A directory removal may be reported only for the parent. Include its
    // previously known descendants so Parcel receives one delete per entry.
    let removed_dirs: Vec<String> = touched
        .iter()
        .filter(|path| snapshot_entry(path).is_none())
        .filter_map(|path| snapshot_key(root, path))
        .filter(|key| snapshot.get(key).is_some_and(|entry| entry.is_dir))
        .collect();
    for dir in removed_dirs {
        let prefix = format!("{dir}/");
        touched.extend(
            snapshot
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .map(|key| root.join(key)),
        );
    }

    let mut events = touched
        .into_iter()
        .filter_map(|path| {
            let existed = path == root
                || snapshot_key(root, &path).is_some_and(|key| snapshot.contains_key(&key));
            let exists = snapshot_entry(&path).is_some();
            let kind = match (existed, exists) {
                (false, true) => CoalescedKind::Create,
                (true, false) => CoalescedKind::Delete,
                (true, true) if path != root => CoalescedKind::Update,
                // Created and deleted within the batch, or a backend's
                // metadata-only notification for the still-live watch root.
                _ => return None,
            };
            Some(ParcelEvent { path, kind })
        })
        .collect::<Vec<_>>();
    // A rename must always expose delete-old before create-new, regardless
    // of the two path names' lexical order.
    events.sort_by(|left, right| {
        let rank = |kind| i32::from(kind != CoalescedKind::Delete);
        rank(left.kind)
            .cmp(&rank(right.kind))
            .then_with(|| left.path.cmp(&right.path))
    });
    events
}

fn update_subscription_snapshot(subscription: &mut Subscription, events: &[ParcelEvent]) {
    for event in events {
        let Some(key) = snapshot_key(&subscription.root, &event.path) else {
            continue;
        };
        match event.kind {
            CoalescedKind::Delete => {
                subscription.snapshot.remove(&key);
            }
            CoalescedKind::Create | CoalescedKind::Update => {
                if let Some(entry) = snapshot_entry(&event.path) {
                    subscription.snapshot.insert(key, entry);
                }
            }
        }
    }
}

fn event_array(events: &[ParcelEvent]) -> f64 {
    let scope = TransientRootScope::enter();
    let array = unsafe { js_array_alloc(events.len() as u32) };
    let rooted_array = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(array).bits()));
    let (packed, shape) = build_object_shape(&["path", "type"]);
    for event in events {
        let path = alloc_string(&event.path.to_string_lossy());
        let path = scope.root_nanbox(f64::from_bits(
            JsValue::from_string_ptr(path.as_raw()).bits(),
        ));
        let kind = alloc_string(match event.kind {
            CoalescedKind::Create => "create",
            CoalescedKind::Update => "update",
            CoalescedKind::Delete => "delete",
        });
        let kind = scope.root_nanbox(f64::from_bits(
            JsValue::from_string_ptr(kind.as_raw()).bits(),
        ));
        let object =
            unsafe { js_object_alloc_with_shape(shape, 2, packed.as_ptr(), packed.len() as u32) };
        unsafe {
            js_object_set_field(object, 0, JsValue::from_bits(path.get().to_bits()));
            js_object_set_field(object, 1, JsValue::from_bits(kind.get().to_bits()));
            let current = JsValue::from_bits(rooted_array.get().to_bits()).as_pointer();
            let _ = js_array_push(current, JsValue::from_object_ptr(object));
        }
    }
    rooted_array.get()
}

fn fire_events(id: u64, events: Vec<ParcelEvent>) -> bool {
    if events.is_empty() {
        return false;
    }
    let scope = TransientRootScope::enter();
    let events_value = scope.root_nanbox(event_array(&events));
    let callback = SUBSCRIPTIONS
        .lock()
        .ok()
        .and_then(|subscriptions| subscriptions.get(&id).map(|entry| entry.callback_handle))
        .and_then(|handle| get_handle::<CallbackRoot>(handle).map(|root| root.callback));
    let Some(callback) = callback else {
        return false;
    };
    let callback = scope.root_addr(callback);
    let closure = unsafe { JsClosure::from_raw(callback.get() as *const RawClosureHeader) };
    unsafe {
        closure.call2(f64::from_bits(JsValue::NULL.bits()), events_value.get());
    }
    true
}

fn fire_error(id: u64, message: &str) -> bool {
    let scope = TransientRootScope::enter();
    let error = error_value_with_code(message, "ERR_PARCEL_WATCHER", ErrorKind::Error);
    let error = scope.root_nanbox(f64::from_bits(error.bits()));
    let empty = unsafe { js_array_alloc(0) };
    let events = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(empty).bits()));
    let callback = SUBSCRIPTIONS
        .lock()
        .ok()
        .and_then(|subscriptions| subscriptions.get(&id).map(|entry| entry.callback_handle))
        .and_then(|handle| get_handle::<CallbackRoot>(handle).map(|root| root.callback));
    let Some(callback) = callback else {
        return false;
    };
    let callback = scope.root_addr(callback);
    let closure = unsafe { JsClosure::from_raw(callback.get() as *const RawClosureHeader) };
    unsafe {
        closure.call2(error.get(), events.get());
    }
    true
}

/// Drain OS watcher events and invoke subscription callbacks on the JS thread.
#[no_mangle]
pub extern "C" fn js_parcel_watcher_process_pending() -> i32 {
    let now = Instant::now();
    let ready = {
        let mut queue = PENDING.lock().unwrap();
        let mut ready = Vec::new();
        let mut waiting = Vec::new();
        for item in std::mem::take(&mut *queue) {
            if now.duration_since(item.queued_at) >= BATCH_WINDOW {
                ready.push(item.event);
            } else {
                waiting.push(item);
            }
        }
        *queue = waiting;
        ready
    };
    if !PENDING.lock().unwrap().is_empty() {
        schedule_batch_wake();
    }
    let mut changes_by_id = BTreeMap::<u64, Vec<Change>>::new();
    let mut errors = Vec::new();
    for item in ready {
        match item {
            Pending::Changes(id, changes) => changes_by_id.entry(id).or_default().extend(changes),
            Pending::Error(id, message) => errors.push((id, message)),
        }
    }

    let mut fired = 0;
    for (id, changes) in changes_by_id {
        let events = {
            let mut subscriptions = SUBSCRIPTIONS.lock().unwrap();
            let Some(subscription) = subscriptions.get_mut(&id) else {
                continue;
            };
            if changes
                .iter()
                .any(|change| matches!(change, Change::Rescan))
            {
                match snapshot_tree(&subscription.root, &subscription.matcher) {
                    Ok(next) => {
                        let events =
                            diff_snapshots(&subscription.root, &subscription.snapshot, &next);
                        subscription.snapshot = next;
                        events
                    }
                    Err(error) => {
                        errors.push((id, error));
                        Vec::new()
                    }
                }
            } else {
                let events = coalesce(
                    &subscription.root,
                    &subscription.matcher,
                    &subscription.snapshot,
                    &changes,
                );
                update_subscription_snapshot(subscription, &events);
                events
            }
        };
        if fire_events(id, events) {
            fired += 1;
        }
    }
    for (id, error) in errors {
        if fire_error(id, &error) {
            fired += 1;
        }
    }
    fired
}

/// Return non-zero while a live subscription owns an OS watcher.
#[no_mangle]
pub extern "C" fn js_parcel_watcher_has_active() -> i32 {
    SUBSCRIPTIONS
        .lock()
        .map(|subscriptions| i32::from(!subscriptions.is_empty()))
        .unwrap_or(0)
}

/// Test/gate probe proving events came from notify's native backend.
#[no_mangle]
pub extern "C" fn js_parcel_watcher_native_event_count() -> f64 {
    NATIVE_EVENT_COUNT.load(Ordering::Relaxed) as f64
}

/// `binding.subscribe(dir, callback, options) -> Promise<void>`.
///
/// # Safety
///
/// `dir` must be null or point to a live Perry string header, and `callback`
/// and `options` must be values produced by the Perry runtime.
#[no_mangle]
pub unsafe extern "C" fn js_parcel_watcher_subscribe(
    dir: *const StringHeader,
    callback: i64,
    options: f64,
) -> *mut Promise {
    ensure_registered();
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    if callback == 0 {
        promise.reject_string("@parcel/watcher subscribe callback must be a function");
        return raw;
    }
    let Some(dir) = read_ptr_string(dir) else {
        promise.reject_string("@parcel/watcher directory must be a string");
        return raw;
    };
    let root = normalize_root(PathBuf::from(dir));
    if !root.is_dir() {
        promise.reject_string(&format!(
            "Unable to watch {}: not a directory",
            root.display()
        ));
        return raw;
    }
    let options = parse_options(options);
    let matcher = match IgnoreMatcher::compile(&options) {
        Ok(matcher) => matcher,
        Err(error) => {
            promise.reject_string(&error);
            return raw;
        }
    };
    let snapshot = match snapshot_tree(&root, &matcher) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            promise.reject_string(&error);
            return raw;
        }
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let active = Arc::new(AtomicBool::new(true));
    let callback_active = active.clone();
    let mut watcher = match notify::recommended_watcher(move |result: notify::Result<Event>| {
        if !callback_active.load(Ordering::Acquire) {
            return;
        }
        match result {
            Ok(event) => {
                NATIVE_EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
                let changes = changes_from_notify(event);
                if !changes.is_empty() {
                    queue_pending(Pending::Changes(id, changes));
                }
            }
            Err(error) => queue_pending(Pending::Error(id, error.to_string())),
        }
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            promise.reject_string(&format!("Unable to create watcher: {error}"));
            return raw;
        }
    };
    if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
        promise.reject_string(&format!("Unable to watch {}: {error}", root.display()));
        return raw;
    }
    let callback_handle = register_handle(CallbackRoot { callback });
    SUBSCRIPTIONS.lock().unwrap().insert(
        id,
        Subscription {
            _watcher: watcher,
            root,
            callback_handle,
            options,
            matcher,
            active,
            snapshot,
        },
    );
    promise.resolve_undefined();
    raw
}

/// `binding.unsubscribe(dir, callback, options) -> Promise<void>`.
///
/// # Safety
///
/// `dir` must be null or point to a live Perry string header, and `callback`
/// and `options` must be values produced by the Perry runtime.
#[no_mangle]
pub unsafe extern "C" fn js_parcel_watcher_unsubscribe(
    dir: *const StringHeader,
    callback: i64,
    options: f64,
) -> *mut Promise {
    ensure_registered();
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(dir) = read_ptr_string(dir) else {
        promise.reject_string("@parcel/watcher directory must be a string");
        return raw;
    };
    let root = normalize_root(PathBuf::from(dir));
    let options = parse_options(options);
    let id = SUBSCRIPTIONS.lock().ok().and_then(|subscriptions| {
        subscriptions.iter().find_map(|(id, subscription)| {
            let same_callback = get_handle::<CallbackRoot>(subscription.callback_handle)
                .is_some_and(|root| root.callback == callback);
            (subscription.root == root && same_callback && subscription.options == options)
                .then_some(*id)
        })
    });
    if let Some(id) = id {
        let removed = SUBSCRIPTIONS.lock().unwrap().remove(&id);
        if let Some(subscription) = removed {
            let callback_handle = subscription.callback_handle;
            subscription.active.store(false, Ordering::Release);
            // Drop synchronously: notify stops and joins before the callback
            // root is released. Then discard anything already queued.
            drop(subscription);
            PENDING.lock().unwrap().retain(|pending| match pending {
                PendingItem {
                    event: Pending::Changes(pending_id, _) | Pending::Error(pending_id, _),
                    ..
                } => *pending_id != id,
            });
            drop_handle(callback_handle);
        }
    }
    promise.resolve_undefined();
    raw
}

fn snapshot_options(options: f64) -> Result<(WatchOptionsKey, IgnoreMatcher), String> {
    let key = unsafe { parse_options(options) };
    let matcher = IgnoreMatcher::compile(&key)?;
    Ok((key, matcher))
}

/// `binding.writeSnapshot(dir, snapshotPath, options) -> Promise<void>`.
///
/// # Safety
///
/// Both string pointers must be null or point to live Perry string headers,
/// and `options` must be a value produced by the Perry runtime.
#[no_mangle]
pub unsafe extern "C" fn js_parcel_watcher_write_snapshot(
    dir: *const StringHeader,
    snapshot_path: *const StringHeader,
    options: f64,
) -> *mut Promise {
    ensure_registered();
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(dir) = read_ptr_string(dir) else {
        promise.reject_string("@parcel/watcher directory must be a string");
        return raw;
    };
    let Some(snapshot_path) = read_ptr_string(snapshot_path) else {
        promise.reject_string("@parcel/watcher snapshot path must be a string");
        return raw;
    };
    let (_, matcher) = match snapshot_options(options) {
        Ok(options) => options,
        Err(error) => {
            promise.reject_string(&error);
            return raw;
        }
    };
    let root = normalize_root(PathBuf::from(dir));
    match snapshot_tree(&root, &matcher)
        .and_then(|snapshot| serde_json::to_vec(&snapshot).map_err(|error| error.to_string()))
        .and_then(|bytes| fs::write(&snapshot_path, bytes).map_err(|error| error.to_string()))
    {
        Ok(()) => promise.resolve_undefined(),
        Err(error) => promise.reject_string(&error),
    }
    raw
}

/// `binding.getEventsSince(dir, snapshotPath, options) -> Promise<Event[]>`.
///
/// # Safety
///
/// Both string pointers must be null or point to live Perry string headers,
/// and `options` must be a value produced by the Perry runtime.
#[no_mangle]
pub unsafe extern "C" fn js_parcel_watcher_get_events_since(
    dir: *const StringHeader,
    snapshot_path: *const StringHeader,
    options: f64,
) -> *mut Promise {
    ensure_registered();
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(dir) = read_ptr_string(dir) else {
        promise.reject_string("@parcel/watcher directory must be a string");
        return raw;
    };
    let Some(snapshot_path) = read_ptr_string(snapshot_path) else {
        promise.reject_string("@parcel/watcher snapshot path must be a string");
        return raw;
    };
    let (_, matcher) = match snapshot_options(options) {
        Ok(options) => options,
        Err(error) => {
            promise.reject_string(&error);
            return raw;
        }
    };
    let root = normalize_root(PathBuf::from(dir));
    let result = fs::read(&snapshot_path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<Snapshot>(&bytes).map_err(|error| error.to_string())
        })
        .and_then(|old| {
            snapshot_tree(&root, &matcher).map(|new| diff_snapshots(&root, &old, &new))
        });
    match result {
        Ok(events) => {
            let scope = TransientRootScope::enter();
            let value = scope.root_nanbox(event_array(&events));
            promise.resolve(JsValue::from_bits(value.get().to_bits()));
        }
        Err(error) => promise.reject_string(&error),
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalescing_matches_parcel_batch_semantics() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let matcher = IgnoreMatcher {
            paths: Vec::new(),
            globs: Vec::new(),
        };
        let created = root.join("created.ts");
        let transient = root.join("transient.ts");
        fs::write(&created, "updated").unwrap();
        let events = coalesce(
            &root,
            &matcher,
            &Snapshot::new(),
            &[
                Change::Create(created.clone()),
                Change::Update(created.clone()),
                Change::Create(transient.clone()),
                Change::Delete(transient),
            ],
        );
        assert_eq!(
            events,
            vec![ParcelEvent {
                path: created,
                kind: CoalescedKind::Create,
            }]
        );
    }

    #[test]
    fn backend_create_for_existing_path_normalizes_to_update() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let path = root.join("existing.ts");
        fs::write(&path, "one").unwrap();
        let matcher = IgnoreMatcher {
            paths: Vec::new(),
            globs: Vec::new(),
        };
        let snapshot = snapshot_tree(&root, &matcher).unwrap();
        fs::write(&path, "two-two").unwrap();
        assert_eq!(
            coalesce(&root, &matcher, &snapshot, &[Change::Create(path.clone())],),
            vec![ParcelEvent {
                path,
                kind: CoalescedKind::Update,
            }]
        );
    }

    #[test]
    fn coalesced_rename_deletes_before_lexically_earlier_create() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let old = root.join("z-old.ts");
        let new = root.join("a-new.ts");
        fs::write(&old, "one").unwrap();
        let matcher = IgnoreMatcher {
            paths: Vec::new(),
            globs: Vec::new(),
        };
        let snapshot = snapshot_tree(&root, &matcher).unwrap();
        fs::rename(&old, &new).unwrap();
        assert_eq!(
            coalesce(
                &root,
                &matcher,
                &snapshot,
                &[Change::Delete(old.clone()), Change::Create(new.clone())],
            ),
            vec![
                ParcelEvent {
                    path: old,
                    kind: CoalescedKind::Delete,
                },
                ParcelEvent {
                    path: new,
                    kind: CoalescedKind::Create,
                },
            ]
        );
    }

    #[test]
    fn rename_is_delete_then_create() {
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from("old"))
            .add_path(PathBuf::from("new"));
        assert!(matches!(
            changes_from_notify(event).as_slice(),
            [Change::Delete(old), Change::Create(new)] if old == Path::new("old") && new == Path::new("new")
        ));
    }

    #[test]
    fn snapshots_round_trip_and_honor_ignores() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("nested")).unwrap();
        fs::write(temp.path().join("nested/kept.txt"), "one").unwrap();
        fs::write(temp.path().join("ignored.txt"), "one").unwrap();
        let matcher = IgnoreMatcher {
            paths: vec![temp.path().join("ignored.txt")],
            globs: Vec::new(),
        };
        let old = snapshot_tree(temp.path(), &matcher).unwrap();
        let encoded = serde_json::to_vec(&old).unwrap();
        let decoded: Snapshot = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(old, decoded);
        assert!(!decoded.contains_key("ignored.txt"));

        fs::write(temp.path().join("nested/kept.txt"), "two-two").unwrap();
        fs::write(temp.path().join("nested/new.txt"), "new").unwrap();
        let new = snapshot_tree(temp.path(), &matcher).unwrap();
        let events = diff_snapshots(temp.path(), &old, &new);
        assert!(events.iter().any(|event| {
            event.path.ends_with("nested/kept.txt") && event.kind == CoalescedKind::Update
        }));
        assert!(events.iter().any(|event| {
            event.path.ends_with("nested/new.txt") && event.kind == CoalescedKind::Create
        }));
    }

    #[test]
    fn snapshot_rename_is_delete_then_create() {
        let root = Path::new("/tmp/project");
        let entry = SnapshotEntry {
            is_dir: false,
            len: 3,
            modified_ns: 1,
        };
        let old = BTreeMap::from([("old.ts".to_string(), entry.clone())]);
        let new = BTreeMap::from([("new.ts".to_string(), entry)]);
        assert_eq!(
            diff_snapshots(root, &old, &new),
            vec![
                ParcelEvent {
                    path: root.join("old.ts"),
                    kind: CoalescedKind::Delete,
                },
                ParcelEvent {
                    path: root.join("new.ts"),
                    kind: CoalescedKind::Create,
                },
            ]
        );
    }

    #[test]
    fn regex_sources_match_root_relative_dotfiles() {
        let root = Path::new("/tmp/project");
        let matcher = IgnoreMatcher {
            paths: Vec::new(),
            globs: vec![Regex::new(r"^(?:.*\.log)$").unwrap()],
        };
        assert!(matcher.ignores(root, &root.join(".cache/debug.log")));
        assert!(!matcher.ignores(root, &root.join("src/main.ts")));
    }
}
