/// Descriptor record indices are below this (the static band's upper half).
const LAYOUT_INDEX_LIMIT: u32 = crate::codegen_abi::STATIC_SHAPE_ID_COUNT / 2;

/// The layout id of descriptor record `index`.
#[inline]
fn layout_id_of_index(index: u32) -> Option<u32> {
    (index < LAYOUT_INDEX_LIMIT).then_some(LAYOUT_BASE + index)
}

/// The descriptor record index a layout word names (shadow bit ignored), or
/// `None` for every word outside the section's ids.
#[inline]
pub(crate) fn layout_index_of_word(word: u32) -> Option<u32> {
    let index = layout_id_of_word(word).wrapping_sub(LAYOUT_BASE);
    let (start, end) = layout_bounds();
    let count = (end.saturating_sub(start) / std::mem::size_of::<PerryLayoutDesc>()) as u32;
    (index < count && index < LAYOUT_INDEX_LIMIT).then_some(index)
}

/// Descriptor `index` of the section.
#[inline]
fn desc_at(index: u32) -> *const PerryLayoutDesc {
    (layout_bounds().0 + index as usize * std::mem::size_of::<PerryLayoutDesc>())
        as *const PerryLayoutDesc
}

/// The class declaration owns the immutable pointer to its instance layout.
/// No descriptor registry or learned class-to-layout mapping.
pub(crate) fn family_of_class(class_id: u32) -> Option<(u32, *const PerryLayoutDesc)> {
    let declaration = super::class_registry::declarations::class_declaration(class_id)?;
    let desc = declaration.instance_layout;
    if desc.is_null() {
        return None;
    }
    let (start, end) = layout_bounds();
    let address = desc as usize;
    if address < start || address >= end {
        return None;
    }
    let index = ((address - start) / std::mem::size_of::<PerryLayoutDesc>()) as u32;
    let parent = unsafe { (*desc).parent_class_id };
    let actual = super::class_meta_registry::get_parent_class_id(class_id).unwrap_or(0);
    (actual == parent).then_some((index, desc))
}

/// Is `d` a layout of class family `class_id` or of a family descending
/// from it (its runtime parents, through their families)?
pub(crate) fn family_descends(d: &PerryLayoutDesc, class_id: u32) -> bool {
    if d.class_id == class_id {
        return true;
    }
    let mut parent = d.parent_class_id;
    for _ in 0..64 {
        if parent == 0 {
            return false;
        }
        if parent == class_id {
            return true;
        }
        parent = match family_of_class(parent) {
            // SAFETY: a registered descriptor of this program.
            Some((_, pd)) => unsafe { (*pd).parent_class_id },
            None => return false,
        };
    }
    false
}

/// A family's creation split (object model §3.2: a family is a creation
/// point), emitted by codegen beside the family's descriptors
/// (`perry-codegen` `layout_desc::emit_module_layouts`): a birth whose
/// constructor argument `param` is a number equal to an entry's constant is
/// born on that entry's layout. Static data, never written.
#[repr(C)]
pub struct PerryFamilyVariants {
    pub param: u32,
    pub count: u32,
    /// `count` entries, sorted by `key_bits`.
    pub entries: [PerryFamilyVariant; 0],
}

/// One layout of a creation split: the constant (an f64's bits) and its
/// descriptor.
#[repr(C)]
pub struct PerryFamilyVariant {
    pub key_bits: u64,
    pub desc: *const PerryLayoutDesc,
}

/// The descriptor a birth of class `class_id` with constructor arguments
/// `args` is born on: its family's creation-split layout for the argument's
/// constant, else the family layout; `None` when the class has no family.
/// Every layout of the family is a correct layout of every instance (each
/// key is a hole until stored), so the argument decides width and slots
/// only.
pub(crate) fn family_birth_desc(class_id: u32, args: &[f64]) -> Option<*const PerryLayoutDesc> {
    let (_, default) = family_of_class(class_id)?;
    let declaration = super::class_registry::declarations::class_declaration(class_id)?;
    let table = declaration.instance_variants;
    if table.is_null() {
        return Some(default);
    }
    // SAFETY: an image constant of this program (codegen emits the table
    // with exactly `count` entries).
    unsafe {
        let t = &*table;
        let Some(&arg) = args.get(t.param as usize) else {
            return Some(default);
        };
        let v = crate::JSValue::from_bits(arg.to_bits());
        if !v.is_number() {
            return Some(default);
        }
        let n = v.as_number();
        // -0 and 0 name one constant; codegen emits integral constants as +0.
        let bits = if n == 0.0 { 0 } else { n.to_bits() };
        let entries = std::slice::from_raw_parts(t.entries.as_ptr(), t.count as usize);
        let Ok(i) = entries.binary_search_by(|e| e.key_bits.cmp(&bits)) else {
            return Some(default);
        };
        let desc = entries[i].desc;
        let (start, end) = layout_bounds();
        let address = desc as usize;
        if address < start || address >= end || (*desc).class_id != class_id {
            return Some(default);
        }
        if family_diag_on() {
            FAMILY_DIAG
                .variant_births
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Some(desc)
    }
}

/// Class births allocate the complete fixed layout directly.
/// Retained for a runtime construction path that already made a keyless shell.
pub(crate) unsafe fn adopt_family_layout(obj: *mut ObjectHeader) -> *mut ObjectHeader {
    if obj.is_null() || is_family_object(obj) {
        return obj;
    }
    let Some((_, desc)) = family_of_class((*obj).class_id) else {
        return obj;
    };
    let d = &*desc;
    let Some(birth) = super::shapes::object_shape_descriptor(obj) else {
        return obj;
    };
    if birth.logical_key_count != 0 || birth.live_inline_slot_count < d.slot_count {
        return obj;
    }
    let prototype = super::shapes::object_prototype_word(obj);
    let brand = super::shapes::object_evaluation_brand(obj);
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(obj);
    let proto = scope.root_nanbox_u64(prototype);
    let brand = scope.root_nanbox_u64(brand);
    // A family object's evaluation brand lives in its meta record
    // (`shapes::object_evaluation_brand`); its [[Prototype]] stays a shape
    // fact, re-linked below once the object is on the layout.
    if brand.get_nanbox_u64() != 0 {
        let (meta, current) =
            object.across_mut::<ObjectHeader, _>(|| super::object_meta_ensure(obj));
        (*meta).private_evaluation_brand = brand.get_nanbox_u64();
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            std::ptr::addr_of!((*meta).private_evaluation_brand) as usize,
            (*meta).private_evaluation_brand,
        );
        let _ = current;
    }
    let obj = object.get_raw_mut_ptr::<ObjectHeader>();
    let fields = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>();
    for i in 0..d.slot_count as usize {
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    super::shapes::stamp_object_shape_id_with_carrier_note(obj, d.layout_id);
    // A birth linked to another prototype (a class evaluation's, a
    // newTarget's) keeps that link as its shape's identity: the funnel moves
    // the object to that evaluation's copy of the layout (or off the family,
    // for a link no class instance identity names).
    let prototype = proto.get_nanbox_u64();
    if prototype != 0 {
        super::prototype_chain::object_link_class_evaluation_prototype(obj as usize, prototype);
    }
    object.get_raw_mut_ptr::<ObjectHeader>()
}

/// A key publication on a static family object
/// (`object::set_object_keys_with_live_rep`) that adds one plain data key its
/// layout does not hold: the layout word stays, the object's overflow
/// (§3.4) is installed and orders it from now on, and the added key's entry
/// references fixed slot `slot_count` (past the layout's inline slots, in the
/// spill), where the caller's add stores the value. Every later key of the
/// object takes the overflow's own routing (`has_overflow`). `false` when
/// this is not such a publication; the caller publishes as before.
///
/// # Safety
/// `obj` is live; `keys` is the receiver's next key list.
pub(crate) unsafe fn route_family_add(
    obj: *mut ObjectHeader,
    keys: super::ObjectKeys,
    live_inline_slot_count: u32,
) -> bool {
    let word = (*obj).parent_class_id;
    if word & LAYOUT_SHADOW_BIT != 0 {
        return false;
    }
    let Some(d) = family_desc_of(obj) else {
        return false;
    };
    if live_inline_slot_count != d.slot_count
        || keys.arr().is_null()
        || keys.count() != d.slot_count + 1
    {
        return false;
    }
    let Some(current) = super::shapes::object_shape_descriptor(obj) else {
        return false;
    };
    if current.keys == 0 {
        return false;
    }
    let old =
        crate::array::array_elements_ptr(current.keys as usize as *const crate::array::ArrayHeader)
            as *const u64;
    let new = crate::array::array_elements_ptr(keys.arr() as *const crate::array::ArrayHeader)
        as *const u64;
    for i in 0..d.slot_count as usize {
        if *old.add(i) != *new.add(i) {
            return false;
        }
    }
    let n = d.slot_count;
    if super::key_attrs::keys_entry(keys.arr(), n) != 0 {
        return false;
    }
    let key = crate::JSValue::from_bits(*new.add(n as usize));
    if !key.is_any_string() && crate::symbol::js_is_symbol(f64::from_bits(key.bits())) == 0 {
        return false;
    }
    super::overflow::append_fixed_ref(obj, key.bits(), n);
    true
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_ALLOC_LAYOUT: unsafe extern "C" fn(*const PerryLayoutDesc) -> *mut ObjectHeader =
    perry_alloc_layout;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_LAYOUT_INDEX_PROBE: unsafe extern "C" fn(
    *const PerryLayoutDesc,
    *const StaticAtom,
) -> i32 = perry_layout_index_probe;

/// Does this word name a compiler-owned class instance layout (the family's
/// own id; an evaluation's copy is an object fact, [`is_family_object`])?
#[inline]
pub(crate) fn is_family_layout_word(word: u32) -> bool {
    layout_index_of_word(word).is_some_and(|i| unsafe {
        let d = &*desc_at(i);
        d.class_id != 0 && registered_layout_id(d.layout_id) == Some(layout_id_of_word(word))
    })
}

// ---------------------------------------------------------------------------
// Case 2c keys (OBJECT_MODEL_2026-10-13.md §3.2, §3.4).
//
// A class family's descriptor lists its constructor keys, then the keys
// static stores add later (late keys, `layout_families::late` in
// perry-codegen), each with `LAYOUT_KEY_LATE` in its attributes, which is the
// key list's `key_attrs::ENTRY_OPTIONAL`. The birth key list names every
// key, and a birth writes none: a key is absent while its slot holds the
// hole and present, at its key position, once written (`key_view`).
//
// A family object keeps its layout word for life. Writing a key is an add
// ([`family_store`] at the runtime's store entries; a constructor's stores
// on a clean chain are plain slot stores in codegen):
//
// * In order (no key after it in the list is present yet: a constructor
//   stores its keys in key order), the write fills the slot in place: the
//   enumeration order is the key sequence.
// * Out of order, the object's overflow (§3.4) is installed first. It is the
//   presence and order authority from then on: the hole slots are its
//   tombstones, and the write appends the key with a reference to its slot.
// * A write that is not a plain add (a non-extensible object, a prototype
//   chain that may intercept it) is the ordinary `[[Set]]`'s: it sees the
//   unwritten key as absent, and its CreateDataProperty step, when it gets
//   there, adds the key through [`family_create`].
//
// A key the layout does not name joins the overflow the same way.
// ---------------------------------------------------------------------------

/// `PerryLayoutKey.attrs` bit of a late key: its slot holds the hole until
/// the key is written. The key list carries it as `key_attrs::ENTRY_OPTIONAL`.
pub const LAYOUT_KEY_LATE: u8 = crate::codegen_abi::LAYOUT_KEY_OPTIONAL;

/// The family descriptor of `obj`'s layout word (shadow bit ignored).
#[inline]
pub(crate) unsafe fn family_desc_of(obj: *const ObjectHeader) -> Option<&'static PerryLayoutDesc> {
    if obj.is_null() || !crate::value::addr_class::is_plausible_heap_addr(obj as usize) {
        return None;
    }
    let word = (*obj).parent_class_id;
    let Some(index) = layout_index_of_word(word) else {
        return evaluation_family_desc_of(word);
    };
    let d = &*desc_at(index);
    (d.class_id != 0 && registered_layout_id(d.layout_id) == Some(layout_id_of_word(word)))
        .then_some(d)
}

/// Is `obj` a class family object: on its family layout, or on a class
/// evaluation's copy of it ([`family_desc_of`])?
///
/// # Safety
/// `obj` is a live object or null.
#[inline]
pub(crate) unsafe fn is_family_object(obj: *const ObjectHeader) -> bool {
    family_desc_of(obj).is_some()
}

/// The family descriptor of an instance of a class EVALUATION (a class
/// expression evaluated more than once): its layout word is not the family's
/// own id but the evaluation's copy of that layout, the family's facts at
/// the identity `MIXED | class | serial` that names the evaluation's
/// prototype. The prototype funnel moves an instance there when it links it
/// (`prototype_chain::object_link_class_evaluation_prototype`,
/// [`adopt_family_layout`]), and the copy's record names the family layout
/// it was taken from ([`note_family_relink`]): the class's layout or one of
/// its creation-split layouts. Only a linked word (its kind bits) can be a
/// copy, so every other receiver answers from its header word.
///
/// # Safety
/// Reads the agent shape directory and this program's descriptor section.
#[inline(always)]
unsafe fn evaluation_family_desc_of(word: u32) -> Option<&'static PerryLayoutDesc> {
    // A copy's identity is `MIXED` (the class and the evaluation prototype's
    // serial), which only ids of the mixed kind name: every other receiver
    // answers from its header word.
    let id = layout_id_of_word(word);
    if super::shapes::shape_word_kind(id) != super::shapes::SHAPE_ID_KIND_MIXED
        || !super::shapes::is_shape_id(id)
    {
        return None;
    }
    evaluation_family_desc_of_record(id)
}

/// [`evaluation_family_desc_of`]'s record read, out of line so the static
/// family answer stays inline in every geometry reader.
#[inline(never)]
unsafe fn evaluation_family_desc_of_record(id: u32) -> Option<&'static PerryLayoutDesc> {
    evaluation_layout_desc(super::shapes::evaluation_layout_family(id))
}

/// The family descriptor a shape record's `evaluation_layout_of` fact names
/// (`shapes::evaluation_layout_family`), or `None` for 0.
///
/// # Safety
/// `family` is a record's `evaluation_layout_of` fact.
#[inline]
pub(crate) unsafe fn evaluation_layout_desc(family: u32) -> Option<&'static PerryLayoutDesc> {
    match family {
        0 => None,
        // The record names a registered class family layout: only
        // [`note_family_relink`] writes it, from one.
        family => {
            debug_assert!(is_family_layout_word(family));
            Some(&*desc_at(family - LAYOUT_BASE))
        }
    }
}

/// Is `word` (an `ObjectHeader` layout word) a static class family layout
/// candidate: in the descriptor section's id range, which only
/// [`family_own_key`] answers without a shape record?
#[inline(always)]
pub(crate) fn word_in_layout_index(word: u32) -> bool {
    word.wrapping_sub(LAYOUT_BASE) < LAYOUT_INDEX_LIMIT
}

/// A prototype transition moved an object from layout word `previous` to
/// ShapeId `id`. When `previous` is a class family layout, or an evaluation
/// copy of one, and `id` has that family's facts at a class evaluation's
/// identity, `id`'s record names the family
/// (`shapes::note_evaluation_layout`), so its objects stay family objects.
pub(crate) fn note_family_relink(previous: u32, id: u32) {
    let previous = layout_id_of_word(previous);
    let family = if is_family_layout_word(previous) {
        previous
    } else if super::shapes::shape_word_may_be_linked(previous) {
        super::shapes::evaluation_layout_family(previous)
    } else {
        0
    };
    if family != 0 {
        super::shapes::note_evaluation_layout(id, family);
    }
}

struct FamilyDiag {
    births: std::sync::atomic::AtomicU64,
    dirty: std::sync::atomic::AtomicU64,
    seen: std::sync::atomic::AtomicU64,
    fills: std::sync::atomic::AtomicU64,
    out_of_order: std::sync::atomic::AtomicU64,
    other: std::sync::atomic::AtomicU64,
    installs: std::sync::atomic::AtomicU64,
    declined: std::sync::atomic::AtomicU64,
    variant_births: std::sync::atomic::AtomicU64,
}

static FAMILY_DIAG: FamilyDiag = FamilyDiag {
    births: std::sync::atomic::AtomicU64::new(0),
    dirty: std::sync::atomic::AtomicU64::new(0),
    seen: std::sync::atomic::AtomicU64::new(0),
    fills: std::sync::atomic::AtomicU64::new(0),
    out_of_order: std::sync::atomic::AtomicU64::new(0),
    other: std::sync::atomic::AtomicU64::new(0),
    installs: std::sync::atomic::AtomicU64::new(0),
    declined: std::sync::atomic::AtomicU64::new(0),
    variant_births: std::sync::atomic::AtomicU64::new(0),
};

/// `PERRY_FAMILY_DIAG=1`: print the family store counters at exit.
pub(crate) fn family_diag_init() {
    per_test_global! {
        static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    }
    extern "C" fn report() {
        family_diag_report();
    }
    if std::env::var_os("PERRY_FAMILY_DIAG").is_some()
        && !ON.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        RUN_DIAG_ON.store(true, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: registering a plain extern "C" function.
        unsafe { libc::atexit(report) };
    }
}

/// `PERRY_FAMILY_DIAG=2`: count out-of-order fills by (family, key written,
/// first later key present) and print the most frequent at exit (measurement
/// only: which key orders the census gets wrong).
pub(crate) fn family_diag_pairs_on() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *ON.get_or_init(|| std::env::var("PERRY_FAMILY_DIAG").as_deref() == Ok("2"))
}

static OUT_OF_ORDER_PAIRS: std::sync::Mutex<
    Option<std::collections::HashMap<(u32, String, String), u64>>,
> = std::sync::Mutex::new(None);

#[cold]
pub(crate) unsafe fn note_out_of_order_pair(obj: *const ObjectHeader, pos: u32) {
    let Some(d) = family_desc_of(obj) else {
        return;
    };
    let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let name = |k: &PerryLayoutKey| {
        atom_bytes(k.atom)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    };
    let Some(later) = keys[pos as usize + 1..]
        .iter()
        .find(|k| *fields.add(k.slot as usize) != crate::value::TAG_HOLE)
    else {
        return;
    };
    let key = (d.class_id, name(&keys[pos as usize]), name(later));
    if let Ok(mut map) = OUT_OF_ORDER_PAIRS.lock() {
        *map.get_or_insert_with(Default::default)
            .entry(key)
            .or_default() += 1;
    }
}

/// `PERRY_FAMILY_DIAG`: count the constructor runs' checks by outcome
/// (`layout_probe::perry_family_run_admits`).
pub(crate) static RUN_DIAG_ON: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub(crate) static RUN_DIAG: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];
/// The outcomes [`RUN_DIAG`] counts.
pub(crate) const RUN_OUTCOMES: [&str; 8] = [
    "admitted",
    "not_family",
    "class",
    "key",
    "flags",
    "order",
    "chain",
    "record",
];

/// Count a run check's outcome `i` ([`RUN_OUTCOMES`]) under `PERRY_FAMILY_DIAG`.
#[inline(always)]
pub(crate) fn note_run(i: usize) {
    if RUN_DIAG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        RUN_DIAG[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn family_diag_report() {
    use std::sync::atomic::Ordering::Relaxed;
    let runs: Vec<String> = RUN_OUTCOMES
        .iter()
        .zip(RUN_DIAG.iter())
        .map(|(n, c)| format!("{n}={}", c.load(Relaxed)))
        .collect();
    eprintln!("PERRY_FAMILY_DIAG runs {}", runs.join(" "));
    if family_diag_pairs_on() {
        if let Ok(map) = OUT_OF_ORDER_PAIRS.lock() {
            let mut pairs: Vec<_> = map.iter().flatten().collect();
            pairs.sort_by(|a, b| b.1.cmp(a.1));
            for ((class_id, key, later), n) in pairs.into_iter().take(40) {
                eprintln!(
                    "PERRY_FAMILY_DIAG out_of_order class={class_id} key={key} after={later} n={n}"
                );
            }
        }
    }
    if std::env::var_os("PERRY_FAMILY_DIAG").is_some() {
        family_diag_other_report();
        eprintln!(
            "PERRY_FAMILY_DIAG births={} variant_births={} dirty={} seen={} fills={} out_of_order={} other={} overflow_installs={} declined={}",
            FAMILY_DIAG.births.load(Relaxed),
            FAMILY_DIAG.variant_births.load(Relaxed),
            FAMILY_DIAG.dirty.load(Relaxed),
            FAMILY_DIAG.seen.load(Relaxed),
            FAMILY_DIAG.fills.load(Relaxed),
            FAMILY_DIAG.out_of_order.load(Relaxed),
            FAMILY_DIAG.other.load(Relaxed),
            FAMILY_DIAG.installs.load(Relaxed),
            FAMILY_DIAG.declined.load(Relaxed)
        );
    }
}

// --- overflow install causes (PERRY_FAMILY_DIAG, measurement only) ---------

/// Why the next `overflow::ensure` on a family object installs its table.
pub(crate) mod install_cause {
    pub const LATE_OUT_OF_ORDER: u8 = 1;
    pub const LATE_PROTO_OR_SPILL: u8 = 2;
    pub const OTHER_KEY: u8 = 3;
    pub const DEFINE: u8 = 4;
    pub const DELETE: u8 = 5;
    pub const DELETE_HOLE: u8 = 6;
    pub const UNIQUE_RUN: u8 = 7;
    pub const NAMES: [&str; 8] = [
        "unknown",
        "late_ooo",
        "late_proto_spill",
        "other_key",
        "define",
        "delete",
        "delete_hole",
        "unique_run",
    ];
}

thread_local! {
    static INSTALL_CAUSE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

static INSTALLS_BY_CAUSE: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];

pub(crate) fn family_diag_on() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *ON.get_or_init(|| std::env::var_os("PERRY_FAMILY_DIAG").is_some())
}

#[inline(always)]
pub(crate) fn note_install_cause(cause: u8) {
    INSTALL_CAUSE.with(|c| c.set(cause));
}

/// `overflow::ensure` is installing a table on `obj`: count it by cause when
/// `obj` is a family object.
pub(crate) unsafe fn diag_note_install(obj: *const ObjectHeader) {
    let cause = INSTALL_CAUSE.with(|c| c.replace(0));
    if family_diag_on() && family_desc_of(obj).is_some() {
        INSTALLS_BY_CAUSE[cause as usize & 7].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// (class id, key) -> (first installs, all stores) of keys a family layout
/// does not name.
static OTHER_KEYS: std::sync::Mutex<Option<std::collections::HashMap<(u32, String), (u64, u64)>>> =
    std::sync::Mutex::new(None);

#[cold]
unsafe fn note_other_key(obj: *const ObjectHeader, key_bits: u64, first: bool) {
    let Some(d) = family_desc_of(obj) else {
        return;
    };
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    if let Ok(mut map) = OTHER_KEYS.lock() {
        let e = map
            .get_or_insert_with(Default::default)
            .entry((d.class_id, name))
            .or_default();
        e.0 += first as u64;
        e.1 += 1;
    }
}

fn family_diag_other_report() {
    use std::sync::atomic::Ordering::Relaxed;
    let causes: Vec<String> = install_cause::NAMES
        .iter()
        .zip(INSTALLS_BY_CAUSE.iter())
        .map(|(n, c)| format!("{n}={}", c.load(Relaxed)))
        .collect();
    eprintln!("PERRY_FAMILY_DIAG install_causes {}", causes.join(" "));
    if std::env::var("PERRY_FAMILY_DIAG").as_deref() != Ok("3") {
        return;
    }
    if let Ok(map) = OTHER_KEYS.lock() {
        let mut rows: Vec<_> = map.iter().flatten().collect();
        let mut by_class: std::collections::HashMap<u32, (u64, u64, u32)> = Default::default();
        for ((c, _), (f, t)) in &rows {
            let e = by_class.entry(*c).or_default();
            e.0 += f;
            e.1 += t;
            e.2 += 1;
        }
        let mut classes: Vec<_> = by_class.into_iter().collect();
        classes.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
        for (c, (f, t, n)) in classes.into_iter().take(20) {
            let name = super::class_name_for_id(c).unwrap_or_default();
            eprintln!(
                "PERRY_FAMILY_DIAG other_class class={c} name={name} first={f} stores={t} keys={n}"
            );
        }
        rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(b.1 .1.cmp(&a.1 .1)));
        for ((c, k), (f, t)) in rows.into_iter().take(80) {
            eprintln!("PERRY_FAMILY_DIAG other class={c} key={k} first={f} stores={t}");
        }
    }
}

/// The word in storage slot `slot` of `obj` (a declared layout's births
/// allocate its slots inline; a slot past the inline storage is the spill's),
/// or `0` (not the hole) when the object has no storage for it.
#[inline(always)]
unsafe fn family_slot_word(obj: *const ObjectHeader, slot: u32) -> u64 {
    if slot < super::object_inline_capacity(obj) {
        return *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
            .add(slot as usize);
    }
    super::spill::spill_get_present(obj as usize, slot as usize).unwrap_or(0)
}

/// Is a key after position `pos` of `obj`'s list present (its slot
/// written)? Then a fill of `pos` in place would enumerate out of order.
pub(crate) unsafe fn later_late_key_present(
    obj: *const ObjectHeader,
    keys: *const crate::array::ArrayHeader,
    count: u32,
    pos: u32,
) -> bool {
    // A family object: its descriptor names every key's slot, all inline (a
    // birth allocates them), so the scan is one load per key.
    if let Some(d) = family_desc_of(obj) {
        if d.key_count == count
            && !d.keys.is_null()
            && d.slot_count <= super::object_inline_capacity(obj)
        {
            let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
            return std::slice::from_raw_parts(d.keys, d.key_count as usize)[pos as usize + 1..]
                .iter()
                .any(|k| *fields.add(k.slot as usize) != crate::value::TAG_HOLE);
        }
    }
    // Any present key after `pos` (a required one always is, an optional or
    // an added one once written): node appends a new key after it.
    (pos + 1..count).any(|p| {
        family_slot_word(obj, super::key_attrs::key_storage_slot(keys, p)) != crate::value::TAG_HOLE
    })
}

/// A plain `[[Set]]` of string key `key_bits` (its bytes `bytes`) with
/// `value` on `obj`, which is its own receiver, when the store adds a key to
/// a layout's own terms ([`family_add`]): an optional key whose slot holds
/// the hole (a family's late key, a runtime layout's optional member), or a
/// key a family layout does not name. `false` leaves the store to the
/// caller's ordinary path: not a family object, a present key, a key the
/// overflow holds, a constructor key, and an add that is not plain (a
/// non-extensible object, a prototype chain that may intercept it). That path
/// sees an unwritten late key as absent (`key_view`, `proxy::own_set_descriptor`)
/// and, when it creates the property, comes back through
/// [`family_create`].
///
/// # Safety
/// `obj` is a live object or null; `key_bits` a live string value whose text
/// is `bytes`; the caller holds no other raw heap pointer across the call
/// (the overflow may allocate).
pub(crate) unsafe fn family_store(
    obj: *mut ObjectHeader,
    key_bits: u64,
    bytes: &[u8],
    value: u64,
) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let family = family_desc_of(obj).is_some();
    if !family && !super::key_view::object_may_have_holes(obj) {
        return false;
    }
    // A private field's by-name write is the private path's: it fills or
    // overwrites the element, never a property of that spelling.
    if super::field_get_set::private_namespace_bytes(bytes) {
        return false;
    }
    FAMILY_DIAG.seen.fetch_add(1, Relaxed);
    if family && family_overwrite(obj, bytes, value) {
        return true;
    }
    let add = match family_add_kind(obj, bytes) {
        // A key outside a layout that is not a family's: the ordinary add.
        Some(FamilyAdd::Other) if !family => return false,
        Some(add) => add,
        None => return false,
    };
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    let receiver =
        f64::from_bits(crate::value::POINTER_TAG | obj as u64 & crate::value::POINTER_MASK);
    if (*gc)._reserved
        & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND)
        != 0
        || super::shape_store::chain_blocks(receiver, bytes)
    {
        FAMILY_DIAG.declined.fetch_add(1, Relaxed);
        return false;
    }
    family_add(obj, key_bits, add, value);
    true
}

/// The `CreateDataProperty` step of an ordinary `[[Set]]` whose receiver
/// `receiver` does not hold key `key` (a string value): the caller's walk has
/// decided the add (the chain does not intercept it, the receiver is
/// extensible). For a family object, and for a layout's optional key whose
/// slot holds the hole on any object with such a layout, this makes the add
/// ([`family_add`]); `false` otherwise, and the caller defines the property.
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_create(receiver: f64, key: f64, value: f64) -> bool {
    let bits = receiver.to_bits();
    let key_value = crate::JSValue::from_bits(key.to_bits());
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG || !key_value.is_any_string()
    {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut ObjectHeader;
    let family = family_desc_of(obj).is_some();
    if !family && !super::key_view::object_may_have_holes(obj) {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(key_value, &mut sso) else {
        return false;
    };
    let add = match family_add_kind(obj, bytes) {
        Some(FamilyAdd::Other) if !family => return false,
        Some(add) => add,
        None => return false,
    };
    family_add(obj, key.to_bits(), add, value.to_bits());
    true
}

/// The position of the key spelled `bytes` in descriptor `d`'s key sequence:
/// one probe of the descriptor's immutable hash index (§3.3), no key-list
/// scan.
#[inline]
unsafe fn desc_position_by_bytes(d: &PerryLayoutDesc, bytes: &[u8]) -> Option<u32> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    desc_position_hashed(d, bytes, super::key_bytes_hash(bytes.as_ptr(), bytes.len()))
}

/// [`desc_position_by_bytes`] for `bytes` whose hash is `hash`.
#[inline]
pub(crate) unsafe fn desc_position_hashed(
    d: &PerryLayoutDesc,
    bytes: &[u8],
    hash: u64,
) -> Option<u32> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    let mask = d.index_mask as u64;
    let mut at = hash & mask;
    for _ in 0..=mask {
        let pos = *d.index.add(at as usize);
        if pos == u32::MAX {
            return None;
        }
        if pos < d.key_count && {
            let atom = (*d.keys.add(pos as usize)).atom;
            (*atom).hash == hash && atom_bytes(atom) == Some(bytes)
        } {
            return Some(pos);
        }
        at = (at + 1) & mask;
    }
    None
}

/// `obj`'s own key spelled `bytes`, answered by its family layout alone
/// when `obj` is a family object with no overflow (every own key is then a
/// layout key, present while its slot holds a value): `Some(Some((pos,
/// slot)))` when the layout names the key at key position `pos` and storage
/// slot `slot` (the slot may hold the hole: the key is absent), `Some(None)`
/// when it names no such key (no own key of that name), `None` when `obj` is
/// not such an object. One probe of the descriptor's immutable hash index:
/// no key-list scan. Allocation-free.
///
/// # Safety
/// `obj` is a live object.
#[inline]
pub(crate) unsafe fn family_own_key(
    obj: *const ObjectHeader,
    bytes: &[u8],
) -> Option<Option<(u32, u32)>> {
    family_own_key_hashed(obj, bytes, None)
}

/// [`family_own_key`] with the key's hash (`key_bytes_hash(bytes)`) when the
/// caller has it already (a fixed key's compile-time hash).
///
/// # Safety
/// As [`family_own_key`]; `hash`, when given, is `bytes`' hash.
#[inline]
pub(crate) unsafe fn family_own_key_hashed(
    obj: *const ObjectHeader,
    bytes: &[u8],
    hash: Option<u64>,
) -> Option<Option<(u32, u32)>> {
    // One compare first: a word in the section's layout ids without the
    // shadow bit (an overflow sets it). Most objects are not family objects.
    let word = (*obj).parent_class_id;
    let index = word.wrapping_sub(LAYOUT_BASE);
    let d = if index < LAYOUT_INDEX_LIMIT {
        let (start, end) = layout_bounds();
        if index as usize >= end.saturating_sub(start) / std::mem::size_of::<PerryLayoutDesc>() {
            return None;
        }
        let d = &*desc_at(index);
        if d.class_id == 0 || registered_layout_id(d.layout_id) != Some(word) {
            return None;
        }
        d
    } else if word & LAYOUT_SHADOW_BIT == 0 {
        // A class evaluation's copy of the layout: the same keys and slots.
        evaluation_family_desc_of(word)?
    } else {
        return None;
    };
    desc_own_key(d, bytes, hash)
}

/// [`family_own_key_hashed`] of an object whose family descriptor the caller
/// already holds (`d`, from its layout word or its shape record).
///
/// # Safety
/// `d` is a registered class family descriptor; `hash`, when given, is
/// `bytes`' hash.
#[inline(always)]
pub(crate) unsafe fn desc_own_key(
    d: &PerryLayoutDesc,
    bytes: &[u8],
    hash: Option<u64>,
) -> Option<Option<(u32, u32)>> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
    // A private element is no own property (the index omits it too). A
    // layout lists each spelling once, so only a match is checked.
    let own = |pos: usize, k: &PerryLayoutKey| {
        (k.attrs & super::key_attrs::ENTRY_PRIVATE == 0).then_some((pos as u32, k.slot))
    };
    if let Some(pos) = leading_position(bytes).filter(|&pos| (pos as usize) < keys.len()) {
        let k = &keys[pos as usize];
        if atom_bytes(k.atom) == Some(bytes) {
            return Some(own(pos as usize, k));
        }
    }
    // A short list: its keys' lengths, then their bytes, with no hashing.
    if keys.len() <= SHORT_FAMILY_KEYS {
        for (pos, k) in keys.iter().enumerate() {
            if atom_bytes(k.atom) == Some(bytes) {
                return Some(own(pos, k));
            }
        }
        return Some(None);
    }
    let hash = hash.unwrap_or_else(|| super::key_bytes_hash(bytes.as_ptr(), bytes.len()));
    Some(desc_position_hashed(d, bytes, hash).map(|pos| (pos, keys[pos as usize].slot)))
}

/// A family layout with at most this many keys is searched by comparing
/// them, cheaper than hashing the name for the index.
const SHORT_FAMILY_KEYS: usize = 8;

/// The key position a family layout over node's EventEmitter gives the
/// emitter's state keys: they lead it, in `EventEmitter.init`'s order
/// (`perry-codegen` `layout_families`, the native parent's init keys). Only
/// a first guess for [`family_own_key`], which compares the key there.
#[inline(always)]
fn leading_position(bytes: &[u8]) -> Option<u32> {
    match bytes {
        b"_events" => Some(0),
        b"_eventsCount" => Some(1),
        b"_maxListeners" => Some(2),
        _ => None,
    }
}

/// A store of a present key of family object `obj`: a plain writable data
/// entry of its overflow, or (no overflow) a layout key whose entry has no
/// attribute but optional (every family key is, so no store cache serves it,
/// `key_attrs::entry_is_plain_overwrite`) and whose slot holds a value. The
/// store overwrites in place; `false` for anything else.
unsafe fn family_overwrite(obj: *mut ObjectHeader, bytes: &[u8], value: u64) -> bool {
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return false;
    }
    let table = super::overflow::table(obj);
    if !table.is_null() {
        // The overflow holds every present key (a layout key as a reference
        // to its slot): a live plain data entry is overwritten in place.
        let Some(entry) = super::overflow::find_bytes(table, bytes) else {
            return false;
        };
        let attrs = super::overflow::entry_attrs(table as *const crate::array::ArrayHeader, entry);
        return super::key_attrs::entry_is_plain_writable_data(attrs)
            && super::overflow::store_index(obj, entry, value);
    }
    // A family layout without an overflow: the descriptor's index names the
    // key's slot.
    if let Some(d) = family_desc_of(obj) {
        if d.slot_count <= super::object_inline_capacity(obj) {
            let Some(pos) = desc_position_by_bytes(d, bytes) else {
                return false;
            };
            let k = &*d.keys.add(pos as usize);
            if k.attrs != LAYOUT_KEY_LATE || family_slot_word(obj, k.slot) == crate::value::TAG_HOLE
            {
                return false;
            }
            super::store_object_field_slot(obj, k.slot as usize, value);
            return true;
        }
    }
    let Some(record) = super::shapes::object_shape_record(obj) else {
        return false;
    };
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return false;
    }
    let Some(pos) =
        super::keys_find_property_slot_by_bytes(keys, record.logical_key_count(), bytes)
    else {
        return false;
    };
    if super::key_attrs::keys_entry(keys, pos) != super::key_attrs::ENTRY_OPTIONAL {
        return false;
    }
    let slot = super::key_attrs::key_storage_slot(keys, pos);
    if slot >= super::object_inline_capacity(obj)
        || family_slot_word(obj, slot) == crate::value::TAG_HOLE
    {
        return false;
    }
    super::store_object_field_slot(obj, slot as usize, value);
    true
}

/// How a store of the key spelled `bytes` adds it to family object `obj`, or
/// `None` when it is not an add this module makes (a present key, a key the
/// overflow holds, a constructor key).
#[derive(Clone, Copy)]
enum FamilyAdd {
    /// A late key at list position `pos`, its slot `slot` holding the hole.
    Late { pos: u32, slot: u32 },
    /// A key the layout does not name.
    Other,
}

unsafe fn family_add_kind(obj: *mut ObjectHeader, bytes: &[u8]) -> Option<FamilyAdd> {
    // A family layout without an overflow: the descriptor's index answers.
    if !super::overflow::has_overflow(obj) {
        if let Some(d) = family_desc_of(obj) {
            if d.slot_count <= super::object_inline_capacity(obj) {
                return match desc_position_by_bytes(d, bytes) {
                    Some(pos) => {
                        let k = &*d.keys.add(pos as usize);
                        (k.attrs & LAYOUT_KEY_LATE != 0
                            && family_slot_word(obj, k.slot) == crate::value::TAG_HOLE)
                            .then_some(FamilyAdd::Late { pos, slot: k.slot })
                    }
                    None => Some(FamilyAdd::Other),
                };
            }
        }
    }
    let record = super::shapes::object_shape_record(obj)?;
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    let count = record.logical_key_count();
    let position = if keys.is_null() {
        None
    } else {
        super::keys_find_property_slot_by_bytes(keys, count, bytes)
    };
    let held_by_overflow = || {
        super::overflow::has_overflow(obj)
            && super::overflow::find_bytes(super::overflow::table(obj), bytes).is_some()
    };
    match position {
        Some(pos) => {
            if super::key_attrs::keys_entry(keys, pos) & super::key_attrs::ENTRY_OPTIONAL == 0 {
                return None;
            }
            let slot = super::key_attrs::key_storage_slot(keys, pos);
            if family_slot_word(obj, slot) != crate::value::TAG_HOLE || held_by_overflow() {
                return None;
            }
            Some(FamilyAdd::Late { pos, slot })
        }
        None if held_by_overflow() => None,
        None => Some(FamilyAdd::Other),
    }
}

/// Add the key `key_bits` to family object `obj` with `value` (see the
/// section comment): a late key in order fills its slot in place; out of
/// order, or with the overflow installed, it is appended to the overflow
/// with a reference to its slot; a key the layout does not name is an
/// overflow value.
unsafe fn family_add(obj: *mut ObjectHeader, key_bits: u64, add: FamilyAdd, value: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    let shadowed = super::overflow::has_overflow(obj);
    let mut out_of_order = false;
    if let FamilyAdd::Late { pos, slot } = add {
        if !shadowed {
            let record = super::shapes::object_shape_record(obj).unwrap();
            let keys = record.keys() as usize as *const crate::array::ArrayHeader;
            out_of_order = later_late_key_present(obj, keys, record.logical_key_count(), pos);
            if out_of_order && family_diag_pairs_on() {
                note_out_of_order_pair(obj, pos);
            }
            // In order: the key's position is its enumeration order. An
            // object something inherits from takes the overflow, whose
            // install sets the shadow bit every inherited-read site tests
            // (step 3).
            if !out_of_order
                && slot < super::object_inline_capacity(obj)
                && !(super::proto_validity::any_prototype_marked()
                    && super::proto_validity::object_is_marked_prototype(obj as usize))
            {
                FAMILY_DIAG.fills.fetch_add(1, Relaxed);
                super::store_object_field_slot(obj, slot as usize, value);
                return;
            }
        }
    }
    if family_diag_on() {
        note_install_cause(match add {
            FamilyAdd::Late { .. } if out_of_order => install_cause::LATE_OUT_OF_ORDER,
            FamilyAdd::Late { .. } => install_cause::LATE_PROTO_OR_SPILL,
            FamilyAdd::Other => install_cause::OTHER_KEY,
        });
        if matches!(add, FamilyAdd::Other) {
            note_other_key(obj, key_bits, !shadowed);
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let val = scope.root_nanbox_u64(value);
    let atom = crate::string::property_atom_from_value(key_bits);
    let atom = scope.root_nanbox_u64(atom);
    if !shadowed {
        FAMILY_DIAG.installs.fetch_add(1, Relaxed);
    }
    let atom_ptr =
        || (atom.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    match add {
        FamilyAdd::Late { .. } => {
            if out_of_order {
                FAMILY_DIAG.out_of_order.fetch_add(1, Relaxed);
            }
            // The overflow first (its tombstone for this hole), then the
            // dynamic writer appends the key with a reference to its slot.
            super::overflow::ensure(owner.get_raw_mut_ptr::<ObjectHeader>());
            super::overflow::store_routed(
                owner.get_raw_mut_ptr::<ObjectHeader>(),
                atom_ptr(),
                f64::from_bits(val.get_nanbox_u64()),
            );
        }
        FamilyAdd::Other => {
            FAMILY_DIAG.other.fetch_add(1, Relaxed);
            super::overflow::store_unfixed_routed(
                owner.get_raw_mut_ptr::<ObjectHeader>(),
                atom_ptr(),
                f64::from_bits(val.get_nanbox_u64()),
            );
        }
    }
}

/// [`family_store`] for NaN-boxed operands: `target` is its own receiver and
/// `key` any string value (a heap or a short string).
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_store_value(target: f64, key: f64, value: f64) -> bool {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if family_desc_of(obj).is_none() && !super::key_view::object_may_have_holes(obj) {
        return false;
    }
    let key_value = crate::JSValue::from_bits(key.to_bits());
    if !key_value.is_any_string() {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(key_value, &mut sso) else {
        return false;
    };
    // `family_store` reads the text before its first allocation only; a copy
    // keeps the borrow independent of the key string.
    let mut buf = [0u8; 96];
    let owned;
    let bytes: &[u8] = if bytes.len() <= buf.len() {
        buf[..bytes.len()].copy_from_slice(bytes);
        &buf[..bytes.len()]
    } else {
        owned = bytes.to_vec();
        &owned
    };
    family_store(obj, key.to_bits(), bytes, value.to_bits())
}

/// [`family_store`] for a key passed as a string header.
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_store_str(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    if key.is_null()
        || family_desc_of(obj).is_none() && !super::key_view::object_may_have_holes(obj)
    {
        return false;
    }
    let key_bits = crate::JSValue::string_ptr(key as *mut _).bits();
    family_store_value(
        f64::from_bits(crate::value::POINTER_TAG | obj as u64 & crate::value::POINTER_MASK),
        f64::from_bits(key_bits),
        value,
    )
}

/// `[[DefineOwnProperty]]` of `key` (a property key) on `receiver`, before the
/// definition runs: when the key is a layout's optional key whose slot holds
/// the hole (absent, `key_view`), the object gets its overflow (§3.4), whose
/// tombstone the hole then is. The definition meets an absent key and adds
/// it, in definition order, with exactly the descriptor's attributes. May
/// allocate: the caller holds its operands in handles.
///
/// # Safety
/// `receiver` is a live value.
pub(crate) unsafe fn prepare_hole_define(receiver: u64, key: u64) {
    if receiver & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return;
    }
    let obj = (receiver & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if super::key_view::own_key_is_hole(obj, crate::JSValue::from_bits(key)) {
        note_install_cause(install_cause::DEFINE);
        super::overflow::ensure(obj);
    }
}

// Wasm uses the same records and registration logic; only image enumeration
// differs because relocated data cannot live in Rust's Wasm custom sections.
fn image_atoms() -> impl Iterator<Item = *mut StaticAtom> {
    #[cfg(target_family = "wasm")]
    {
        inventory::iter::<crate::wasm_image::Atom>
            .into_iter()
            .map(|item| item.0.cast_mut())
    }
    #[cfg(not(target_family = "wasm"))]
    {
        records::<StaticAtom>(atom_bounds())
    }
}
fn image_layouts() -> impl Iterator<Item = *mut PerryLayoutDesc> {
    #[cfg(target_family = "wasm")]
    {
        inventory::iter::<crate::wasm_image::Layout>
            .into_iter()
            .map(|item| item.0)
    }
    #[cfg(not(target_family = "wasm"))]
    {
        records::<PerryLayoutDesc>(layout_bounds())
    }
}
