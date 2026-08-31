//! Per-image class registries (#8546).
//!
//! Every class-id-keyed table that codegen populates at module init —
//! vtables, static methods and accessors, constructors, parent edges, names,
//! `.length`s, the `extends Error` / `DataView` / typed-array marks, the
//! `Symbol.hasInstance` / `Symbol.toStringTag` hooks — used to be a
//! process-global `static`. Class ids are assigned by codegen from a small
//! sequential counter, so they identify a class *within one compiled image*
//! and nothing else. A host that dlopens several application images into one
//! process (Coop hosts each deployment on its own dedicated Perry thread) has N
//! images registering the SAME ids with DIFFERENT `func_ptr`s — each image's
//! own code addresses — into one table. `HashMap::insert` is last-writer-wins,
//! so after the last image's init every class of every earlier image
//! dispatched into the last image's code, and only the last-initialised
//! application worked.
//!
//! No write order over a shared table can work: first-wins for methods only
//! leaves every vtable a mix of two images; first-owner for every entry point
//! leaves later images unable to initialise at all (they both write and read
//! these tables during their own init). The tables have to be per image.
//!
//! # The model
//!
//! An **image** is one compiled program's worth of class metadata,
//! [`ClassImageTables`]. A thread resolves its image in this order:
//!
//! 1. the image installed in its own thread-local slot, if any;
//! 2. otherwise the process's **primary** image — the first image ever
//!    created in the process.
//!
//! Installation happens at exactly three points:
//!
//! * [`enter_current_thread_image`], called from `js_gc_init`, which codegen
//!   emits as the first runtime call of both an executable's `main` and a
//!   library's `perry_module_init` — i.e. on whichever thread runs an image's
//!   module init, before any class is registered. The first thread to enter
//!   creates the primary image and owns it; every later thread that enters
//!   gets a fresh, private image. A host loading three applications on three
//!   threads therefore gets three images, and a plain executable gets one.
//!   Idempotent per thread.
//! * [`adopt_image`], on a `perry/thread` worker (`spawn`, `parallelMap`,
//!   `parallelFilter`) and a `worker_threads` Worker, with the handle its
//!   spawner captured via [`current_image_handle`]. Those threads never run
//!   module init (the closure body is all they execute — see `thread.rs`), so
//!   they must SHARE their spawner's tables rather than start empty.
//! * Nothing else. A thread that neither entered nor adopted — a pump running
//!   JS on the primary heap's behalf (Android's UI thread firing timers via
//!   `nativePumpTick`), a reactor thread, a libtest thread — reads and writes
//!   the primary image, which is exactly the process-global table it saw
//!   before this module existed. A program with one image is behaviourally
//!   unchanged.
//!
//! Why not key by `AgentId` or by thread: `CURRENT_AGENT` defaults to
//! `PRIMARY_AGENT` and a host's app thread is a plain `std::thread::spawn`
//! that never claims an agent, so agent-keying hands every hosted app one
//! table (#8528 hit the same wall). Pure thread-keying breaks the pump threads
//! and the `perry/thread` workers above, which run JS that dispatches through
//! these tables without ever running init.
//!
//! # Call sites
//!
//! Each table is a `static` [`ImageTable`] handle whose `read()` / `write()`
//! resolve the calling thread's image and lock that image's `RwLock` — the
//! same guard types the process-global statics handed out, so the ~100 use
//! sites are unchanged. The guards are `!Send`, so a reference into an image
//! cannot leave the thread that resolved it (see [`current`]).
//!
//! The `RegistryLatch`es that gate the slow paths (`HAS_INSTANCE_LATCH`,
//! `GENERIC_ORIGIN_LATCH`, …) and `VTABLE_GEN` stay process-global on
//! purpose: a latch armed by ANY image only ever costs another image the slow
//! path, never a wrong answer.

use crate::fast_hash::{PtrHashMap, PtrHashSet};
use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LockResult, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::ThreadId;

use super::class_registry::ClassVTable;

/// Number of class ids covered by the dense parent table (`parent_dense`).
/// See `object/class_meta_registry.rs` for why the hot parent-edge read is an
/// indexed load rather than a locked hash probe.
pub(crate) const PARENT_DENSE_CAP: usize = 1 << 16;

/// class_id -> { name -> (func_ptr, param_count, has_rest) } for static methods.
///
/// OUTER map only takes the fast hasher (see `ClassImageTables`); the INNER
/// `HashMap<String, _>` stays on SipHash because its keys are JS-supplied
/// member names.
pub type StaticMethodTable = PtrHashMap<u32, HashMap<String, (usize, u32, bool)>>;
/// class_id -> { name -> (getter func_ptr, setter func_ptr) } for static accessors.
/// Outer map fast-hashed, inner `String`-keyed map deliberately not — see above.
pub type StaticAccessorTable = PtrHashMap<u32, HashMap<String, (usize, usize)>>;
/// `(class_id, is_static, property_name) -> source-order token` for declared
/// string-keyed methods and accessors. The token is the member function's HIR
/// id, which is allocated while walking the class body and therefore orders
/// entries across the otherwise separate method/getter/setter registries.
pub type StringMemberOrderTable = HashMap<(u32, bool, String), u32>;
/// class_id -> (ctor func_ptr, total param count, signature capture count).
pub type ConstructorTable = PtrHashMap<u32, (usize, u32, u32)>;
/// class_id -> (has_synthetic_arguments, has_rest) for a registered constructor.
pub type ConstructorFlagTable = PtrHashMap<u32, (bool, bool)>;

/// One compiled image's class metadata: every class-id-keyed table module init
/// writes. Field docs live on the `static` handles that select them.
///
/// # Hasher choice
///
/// The class-id-keyed tables use [`PtrHashMap`] / [`PtrHashSet`]
/// (`PtrHasher`: one multiply plus an avalanche step) rather than std's
/// SipHash. A class id is minted by codegen from a small sequential counter
/// (`perry-hir::lower::context`), so it is a dense process-internal integer
/// and never external input — hash-flooding resistance buys nothing, exactly
/// as for the pointer-keyed registries in [`crate::fast_hash`]. `PtrHasher`'s
/// `write_u32` override (#8125) keeps a bare `u32` key on the single-multiply
/// path, and its avalanche spreads a dense sequential run across buckets.
///
/// Iteration order is not observable for any of these: nothing in the tree
/// iterates them (only `get` / `insert` / `contains`). The `String`-keyed
/// INNER maps of [`StaticMethodTable`] / [`StaticAccessorTable`], and the
/// `(u32, String)`-keyed `method_bind_lengths` pair below, deliberately stay
/// on SipHash — their keys are JS-supplied member names, and the inner maps
/// are enumerated on paths that reach user-visible output.
pub struct ClassImageTables {
    pub(crate) vtables: RwLock<Option<PtrHashMap<u32, ClassVTable>>>,
    pub(crate) static_methods: RwLock<Option<StaticMethodTable>>,
    pub(crate) static_accessors: RwLock<Option<StaticAccessorTable>>,
    pub(crate) string_member_orders: RwLock<Option<StringMemberOrderTable>>,
    pub(crate) method_bind_lengths: RwLock<Option<HashMap<(u32, String), u32>>>,
    pub(crate) static_method_bind_lengths: RwLock<Option<HashMap<(u32, String), u32>>>,
    pub(crate) registered_class_ids: RwLock<Option<PtrHashSet<u32>>>,
    pub(crate) parents: RwLock<Option<PtrHashMap<u32, u32>>>,
    /// `parent + 1` for every registered edge whose child id is
    /// `< PARENT_DENSE_CAP`; `0` means "no edge". Heap-allocated per image
    /// (256 KiB) rather than `.bss`, because there is one per image now.
    pub(crate) parent_dense: Box<[AtomicU32]>,
    pub(crate) fetch_parent_kind: RwLock<Option<PtrHashMap<u32, u8>>>,
    pub(crate) generic_origin: RwLock<Option<PtrHashMap<u32, u32>>>,
    pub(crate) extends_error: RwLock<Option<PtrHashSet<u32>>>,
    pub(crate) has_instance: RwLock<Option<PtrHashMap<u32, usize>>>,
    pub(crate) to_string_tag: RwLock<Option<PtrHashMap<u32, usize>>>,
    pub(crate) constructors: RwLock<Option<ConstructorTable>>,
    pub(crate) constructor_flags: RwLock<Option<ConstructorFlagTable>>,
    pub(crate) extends_data_view: RwLock<Option<PtrHashSet<u32>>>,
    pub(crate) extends_typed_array: RwLock<Option<PtrHashSet<u32>>>,
    pub(crate) names: RwLock<Option<PtrHashMap<u32, String>>>,
    pub(crate) lengths: RwLock<Option<PtrHashMap<u32, u32>>>,
    pub(crate) anon_shape_class_ids: RwLock<Option<PtrHashSet<u32>>>,
}

impl ClassImageTables {
    fn new() -> Self {
        Self {
            vtables: RwLock::new(None),
            static_methods: RwLock::new(None),
            static_accessors: RwLock::new(None),
            string_member_orders: RwLock::new(None),
            method_bind_lengths: RwLock::new(None),
            static_method_bind_lengths: RwLock::new(None),
            registered_class_ids: RwLock::new(None),
            parents: RwLock::new(None),
            parent_dense: (0..PARENT_DENSE_CAP).map(|_| AtomicU32::new(0)).collect(),
            fetch_parent_kind: RwLock::new(None),
            generic_origin: RwLock::new(None),
            extends_error: RwLock::new(None),
            has_instance: RwLock::new(None),
            to_string_tag: RwLock::new(None),
            constructors: RwLock::new(None),
            constructor_flags: RwLock::new(None),
            extends_data_view: RwLock::new(None),
            extends_typed_array: RwLock::new(None),
            names: RwLock::new(None),
            lengths: RwLock::new(None),
            anon_shape_class_ids: RwLock::new(None),
        }
    }
}

/// An owning handle to one image's tables, for handing a spawner's image to
/// the thread it spawns ([`current_image_handle`] → [`adopt_image`]). Opaque:
/// the tables are only ever reached through the `static` [`ImageTable`]
/// handles on the thread that holds the image.
#[derive(Clone)]
pub struct ClassImageHandle(Arc<ClassImageTables>);

impl ClassImageHandle {
    /// Identity of the image behind this handle — two handles compare equal
    /// exactly when they share tables. For tests and diagnostics.
    pub fn image_id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

/// The first image created in this process, and the thread that created it.
/// Never dropped: it is what every thread without an image of its own reads.
struct PrimaryImage {
    tables: Arc<ClassImageTables>,
    owner: ThreadId,
}

static PRIMARY_IMAGE: OnceLock<PrimaryImage> = OnceLock::new();

crate::perry_thread_local! {
    /// This thread's image, once installed by [`enter_current_thread_image`]
    /// or [`adopt_image`]. Set at most once per thread — `current` hands out
    /// references whose validity rests on the slot never being replaced.
    static CURRENT_IMAGE: OnceCell<Arc<ClassImageTables>> = OnceCell::new();
}

fn primary() -> &'static PrimaryImage {
    PRIMARY_IMAGE.get_or_init(|| PrimaryImage {
        tables: Arc::new(ClassImageTables::new()),
        owner: std::thread::current().id(),
    })
}

/// The calling thread's image: its own if one is installed, else the primary.
///
/// The returned reference is valid for the life of the calling thread, not
/// for `'static`: this thread's `Arc` is set once, never replaced, and dropped
/// only in this thread's TLS teardown. Every value derived from it — the
/// `RwLock` guards [`ImageTable::read`] / [`ImageTable::write`] return — is
/// `!Send`, so nothing can carry it to a thread that outlives this one. During
/// TLS teardown `try_with` fails and the primary (never dropped) answers, so a
/// destructor that still consults class metadata reads a live table.
#[inline]
fn current() -> &'static ClassImageTables {
    match CURRENT_IMAGE.try_with(|slot| slot.get().map(Arc::as_ptr)) {
        // SAFETY: see the doc comment — the pointee is owned by this thread's
        // `OnceCell<Arc<_>>`, which is never replaced and outlives every
        // (`!Send`) borrow taken from it on this thread.
        Ok(Some(tables)) => unsafe { &*tables },
        _ => &primary().tables,
    }
}

/// Give the calling thread its own image, unless it already has one.
///
/// Called from `js_gc_init`, i.e. at the top of every `main` /
/// `perry_module_init`, on the thread about to run that image's module init.
/// The first thread to enter creates and owns the primary image (so a thread
/// that touched class metadata before its `js_gc_init` — through the primary
/// fallback — keeps what it wrote); every later thread gets a fresh image.
pub fn enter_current_thread_image() {
    let _ = CURRENT_IMAGE.try_with(|slot| {
        if slot.get().is_some() {
            return;
        }
        let me = std::thread::current().id();
        let primary = primary();
        let tables = if primary.owner == me {
            Arc::clone(&primary.tables)
        } else {
            Arc::new(ClassImageTables::new())
        };
        let _ = slot.set(tables);
    });
}

/// The image the calling thread resolves to, as a handle a spawned thread can
/// [`adopt_image`] before it runs any JS.
pub fn current_image_handle() -> ClassImageHandle {
    let own = CURRENT_IMAGE
        .try_with(|slot| slot.get().cloned())
        .ok()
        .flatten();
    ClassImageHandle(own.unwrap_or_else(|| Arc::clone(&primary().tables)))
}

/// Make the calling thread share `handle`'s tables. Must run before the
/// thread's first class-metadata access; a thread that already has an image
/// keeps it (so a `worker_threads` Worker that adopted its parent's image and
/// then re-runs module init through `js_gc_init` stays on the shared tables).
pub fn adopt_image(handle: ClassImageHandle) {
    let _ = CURRENT_IMAGE.try_with(|slot| {
        let _ = slot.set(handle.0);
    });
}

/// Identity of the image the calling thread currently resolves to.
pub fn current_image_id() -> usize {
    current() as *const ClassImageTables as usize
}

/// A `static` handle selecting one table out of the calling thread's image.
///
/// `read()` / `write()` return the plain `std::sync::RwLock` guards, so a call
/// site written against the former process-global `static RwLock<..>` compiles
/// unchanged.
pub struct ImageTable<T: 'static> {
    select: fn(&ClassImageTables) -> &T,
}

impl<T: 'static> ImageTable<T> {
    pub const fn new(select: fn(&ClassImageTables) -> &T) -> Self {
        Self { select }
    }
}

impl<T: 'static> ImageTable<RwLock<T>> {
    /// Shared-lock this table in the calling thread's image.
    #[inline]
    pub fn read(&'static self) -> LockResult<RwLockReadGuard<'static, T>> {
        (self.select)(current()).read()
    }

    /// Exclusive-lock this table in the calling thread's image.
    #[inline]
    pub fn write(&'static self) -> LockResult<RwLockWriteGuard<'static, T>> {
        (self.select)(current()).write()
    }
}

/// One relaxed-ordering load from the calling image's dense parent table.
/// `idx` must be `< PARENT_DENSE_CAP`.
#[inline]
pub(crate) fn parent_dense_load(idx: usize) -> u32 {
    current().parent_dense[idx].load(Ordering::Acquire)
}

/// Publish one biased parent edge into the calling image's dense table.
/// `idx` must be `< PARENT_DENSE_CAP`.
#[inline]
pub(crate) fn parent_dense_store(idx: usize, biased_parent: u32) {
    current().parent_dense[idx].store(biased_parent, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    /// Register `method` on `class_id` the way codegen's module-init prelude
    /// does, with `func_ptr` standing in for the image's code address.
    unsafe fn register_method(class_id: u32, method: &str, func_ptr: usize) {
        crate::object::class_registry::js_register_class_method(
            class_id as i64,
            method.as_ptr(),
            method.len() as i64,
            func_ptr as i64,
            1,
            0,
            0,
        );
    }

    /// The `func_ptr` dynamic dispatch would call for `class_id.method()`.
    fn dispatch_target(class_id: u32, method: &str) -> Option<usize> {
        crate::object::lookup_class_method_in_chain(class_id, method).map(|(ptr, ..)| ptr)
    }

    /// #8546 — two application images register the SAME class id with
    /// DIFFERENT method addresses, each on its own thread. Both registrations
    /// complete before either thread looks up, so under a shared table the
    /// last writer wins and one thread dispatches into the other image's
    /// code. Each thread must see only its own image's `func_ptr`.
    #[test]
    fn two_application_threads_keep_their_own_class_vtables() {
        const TWO_IMAGES_CLASS_ID: u32 = 0x7d01_8546;
        const METHOD: &str = "m";
        let both_registered = Arc::new(Barrier::new(2));

        let application = |func_ptr: usize, barrier: Arc<Barrier>| {
            std::thread::spawn(move || {
                // What `js_gc_init` does at the top of `perry_module_init`.
                enter_current_thread_image();
                unsafe { register_method(TWO_IMAGES_CLASS_ID, METHOD, func_ptr) };
                barrier.wait();
                (
                    current_image_id(),
                    dispatch_target(TWO_IMAGES_CLASS_ID, METHOD),
                )
            })
        };

        let a = application(0x1000, Arc::clone(&both_registered));
        let b = application(0x2000, both_registered);
        let (a_image, a_target) = a.join().expect("application A panicked");
        let (b_image, b_target) = b.join().expect("application B panicked");

        assert_eq!(
            a_target,
            Some(0x1000),
            "application A dispatches `m` into the other image's code"
        );
        assert_eq!(
            b_target,
            Some(0x2000),
            "application B dispatches `m` into the other image's code"
        );
        assert_ne!(
            a_image, b_image,
            "two entered application threads must hold distinct images"
        );
    }

    /// A `perry/thread` worker never runs module init, so it must adopt its
    /// spawner's image: the spawner's registrations are visible to it, and its
    /// own registrations flow back — while a second application stays out of
    /// reach of both.
    #[test]
    fn a_spawned_worker_shares_its_spawners_image() {
        const SHARED_IMAGE_CLASS_ID: u32 = 0x7d02_8546;
        let (spawner_sees, worker_sees, worker_image, spawner_image) = std::thread::spawn(|| {
            enter_current_thread_image();
            unsafe { register_method(SHARED_IMAGE_CLASS_ID, "spawner", 0x11) };
            let handle = current_image_handle();
            let worker = std::thread::spawn(move || {
                adopt_image(handle);
                unsafe { register_method(SHARED_IMAGE_CLASS_ID, "worker", 0x22) };
                (
                    dispatch_target(SHARED_IMAGE_CLASS_ID, "spawner"),
                    current_image_id(),
                )
            })
            .join()
            .expect("worker panicked");
            (
                dispatch_target(SHARED_IMAGE_CLASS_ID, "worker"),
                worker.0,
                worker.1,
                current_image_id(),
            )
        })
        .join()
        .expect("spawner panicked");

        assert_eq!(
            worker_image, spawner_image,
            "the worker adopted a different image"
        );
        assert_eq!(
            worker_sees,
            Some(0x11),
            "the worker cannot see its spawner's classes"
        );
        assert_eq!(
            spawner_sees,
            Some(0x22),
            "the spawner cannot see its worker's classes"
        );

        // And an unrelated application thread sees neither.
        let other = std::thread::spawn(|| {
            enter_current_thread_image();
            (
                dispatch_target(SHARED_IMAGE_CLASS_ID, "spawner"),
                dispatch_target(SHARED_IMAGE_CLASS_ID, "worker"),
            )
        })
        .join()
        .expect("other application panicked");
        assert_eq!(
            other,
            (None, None),
            "a second application sees the first's classes"
        );
    }

    /// A thread that neither entered nor adopted an image — a pump thread
    /// firing JS on the primary heap's behalf — reads the primary image, which
    /// is where a thread that never called `js_gc_init` also writes. This is
    /// the pre-#8546 process-global behaviour, kept for single-image programs.
    #[test]
    fn a_thread_without_an_image_uses_the_primary() {
        const PRIMARY_IMAGE_CLASS_ID: u32 = 0x7d03_8546;
        // The libtest thread has not entered an image; its write lands in the
        // primary.
        unsafe { register_method(PRIMARY_IMAGE_CLASS_ID, "pump", 0x33) };
        let seen = std::thread::spawn(|| dispatch_target(PRIMARY_IMAGE_CLASS_ID, "pump"))
            .join()
            .expect("pump thread panicked");
        assert_eq!(
            seen,
            Some(0x33),
            "a pump thread must read the primary image"
        );

        // Entering is idempotent: a thread that already resolves to some image
        // keeps it, so a second `js_gc_init` on the same thread is harmless.
        let (before, after) = std::thread::spawn(|| {
            enter_current_thread_image();
            let before = current_image_id();
            enter_current_thread_image();
            (before, current_image_id())
        })
        .join()
        .expect("re-entering thread panicked");
        assert_eq!(before, after, "re-entering replaced the thread's image");
    }
}
