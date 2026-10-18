//! Object model step 5 (OBJECT_MODEL_2026-10-13.md §3.7 case 3): the
//! unknown-receiver probe.
//!
//! A read site whose receiver's layout no compile-time fact names asks the
//! receiver's own layout: the layout word in the header, the layout's record
//! (one address computation, `shapes_store/flat.rs`), the record's key index,
//! the slot; on a proven miss, the overflow (shadow bit) and then the
//! prototype's layout the same way. Nothing is learned or remembered at the
//! site; every word read is an object's own data or immutable layout data, so
//! an answer never goes stale and nothing validates it.
//!
//! # The key index
//!
//! A record's key list is immutable for the record's life (a shape-shared
//! canonical list: only its tip past every published count is ever appended
//! to). The index maps a key's atom hash to its position in that list. It is
//! derived from the list on the first probe of the record and released with
//! the record ([`release_probe`]); being a function of immutable data it is
//! never invalidated. A list of at most [`SCAN_KEYS`] keys has no table: the
//! probe compares its words directly.
//!
//! # What a word compare proves
//!
//! Site keys are property atoms (one per text per agent), so equal words are
//! the same key. A list may hold a plain or short string written before its
//! text's atom existed; the index then holds the text's program atom, or,
//! when there is none, a TEXT entry under the text's hash that the cold path
//! compares by text. So a miss proves absence for every list of strings and
//! symbols (`PROBE_ATOMS`), whatever the record's generation: presence is a
//! fact of the complete key list, not of the record's identity.
//!
//! # What the probe answers
//!
//! An own data property; an overflow entry; a data property of the first
//! prototype that holds the key, when every object before it proves the key
//! absent. Everything else (an accessor, an exotic or per-object receiver, a
//! chain that ends without the key, a builtin prototype that misses) is
//! `TAG_HOLE`, and the site takes its complete slow operation.
use super::shapes_store::{ShapeRecord, FLAT_BASE_OFFSET};
use crate::object::layout_desc::note_run;
use crate::object::ObjectHeader;
use crate::value::TAG_HOLE;

/// Lists of at most this many keys are scanned, not indexed.
pub(crate) const SCAN_KEYS: u32 = 8;
/// Prototype hops the probe follows before it declines.
const MAX_HOPS: usize = 8;

/// The probe word has been derived.
const PROBE_READY: u64 = 1;
/// Every key of the list is a property atom or a symbol: a miss is absence.
const PROBE_ATOMS: u64 = 2;
/// The record's list is not an immutable shape-shared list, or the record's
/// receivers are not answered by their list: the probe declines every key.
const PROBE_DECLINE: u64 = 4;
/// Every probe of the record takes the cold path (verify mode, which holds
/// each answer back for the slow entry's check).
const PROBE_CHECKED: u64 = 1 << 63;
/// The record holds an accessor or a private entry: its list is always
/// indexed, and a hit reads one load only at an entry marked
/// [`DIRECT_ENTRY`]; every other hit checks the key's attributes
/// (`value_at`).
const PROBE_ATTRS: u64 = 1 << 62;
/// A bucket entry whose list word is a TEXT entry (see `derive_word`).
const TEXT_ENTRY: u32 = 1 << 31;
/// A bucket entry matched by the program atom in the index's alias words.
const ALIAS_ENTRY: u32 = 1 << 30;
/// A bucket entry whose key is a data property (no accessor, not private)
/// stored at its own position, an inline slot of every receiver of the
/// record: the hit is one load (`value_at` would read exactly that slot).
/// A fact of the record's immutable list, its attributes and its inline
/// slot count, derived with the index.
const DIRECT_ENTRY: u32 = 1 << 29;
/// The position bits of a bucket entry.
const POS_BITS: u32 = DIRECT_ENTRY - 1;
/// The bits of a probe word that are not the position table's address.
const PROBE_FLAGS: u64 = 7 | PROBE_CHECKED | PROBE_ATTRS;

/// A derived key index: `count` alias words, then `mask + 1` buckets of key
/// positions, `u32::MAX` = empty, open addressing from `hash & mask`. The
/// alias word of a [`TEXT_ENTRY`] position holds its text's hash.
///
/// A bucket compares the site key against the list's own word at its
/// position, which the collector keeps current (a symbol or a plain string
/// key moves with its list). The index holds no copy of a key word, except
/// an [`ALIAS_ENTRY`]'s: a short or plain string list word whose text has a
/// program atom is matched by that atom, which is immortal and never moves.
///
/// After the buckets, `nblocks` words list the hashes of the list's entries
/// that can intercept a `[[Set]]` add of their name on an object inheriting
/// from the record's receivers (an accessor, a non-writable or a private
/// entry), all of them; [`BLOCKS_UNKNOWN`] (and no words) when one has no
/// hash. Like the buckets, a function of the immutable list alone
/// ([`chain_admits_family`] reads it).
#[repr(C)]
struct KeyIndex {
    mask: u32,
    count: u32,
    nblocks: u32,
    _pad: u32,
    words: [u64; 0],
}

/// [`KeyIndex::nblocks`] of a list whose blocking entries it does not list.
const BLOCKS_UNKNOWN: u32 = u32::MAX;

impl KeyIndex {
    #[inline(always)]
    unsafe fn word(this: *const KeyIndex, pos: u32) -> u64 {
        *(*this).words.as_ptr().add(pos as usize)
    }
    #[inline(always)]
    unsafe fn buckets(this: *const KeyIndex) -> *const u32 {
        (*this).words.as_ptr().add((*this).count as usize).cast()
    }
    /// The blocking hashes: after the buckets, whose count is a power of two
    /// of at least two, so the words stay 8-aligned.
    #[inline(always)]
    unsafe fn blocks(this: *const KeyIndex) -> *const u64 {
        Self::buckets(this).add((*this).mask as usize + 1).cast()
    }
    /// The number of blocking hash words the index holds.
    #[inline(always)]
    unsafe fn block_words(this: *const KeyIndex) -> usize {
        match (*this).nblocks {
            BLOCKS_UNKNOWN => 0,
            n => n as usize,
        }
    }
}

const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_NULL: u64 = crate::value::TAG_NULL;

#[inline(always)]
fn hole() -> f64 {
    f64::from_bits(TAG_HOLE)
}

/// Release the position table a probe word owns (record removal).
pub(crate) fn release_probe(word: u64) {
    let table = (word & !PROBE_FLAGS) as usize;
    if table != 0 {
        // SAFETY: allocated by `derive` with exactly this layout.
        unsafe {
            let t = table as *const KeyIndex;
            std::alloc::dealloc(
                table as *mut u8,
                index_layout(
                    (*t).count as usize,
                    (*t).mask as usize + 1,
                    KeyIndex::block_words(t),
                ),
            );
        }
    }
}

fn index_layout(count: usize, buckets: usize, blocks: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(
        std::mem::size_of::<KeyIndex>() + count * 8 + buckets * 4 + blocks * 8,
        8,
    )
    .expect("key index layout")
}

/// The hash a key word is indexed under: a property atom's creation hash
/// (FNV-1a of its bytes, what codegen emits for a site key), a symbol's
/// creation number; `None` for a word no site key can equal.
unsafe fn key_word_hash(bits: u64) -> Option<u64> {
    if bits & TAG_MASK == STRING_TAG {
        let s = (bits & POINTER_MASK) as *const crate::StringHeader;
        if s.is_null() || !crate::string::is_property_atom(s) {
            return None;
        }
        return Some(crate::string::property_atom_hash(s));
    }
    if crate::symbol::js_is_symbol(f64::from_bits(bits)) != 0 {
        return Some((*((bits & POINTER_MASK) as *const crate::symbol::SymbolHeader)).id);
    }
    None
}

/// Derive record `r`'s probe word from its key list (its colour masks are
/// facts of the record from its mint, [`mint_colour_masks`]).
#[cold]
#[inline(never)]
unsafe fn derive(r: *mut ShapeRecord) -> u64 {
    let mut word = derive_word(&*r);
    // Verify mode sends every probe of the record through the cold path,
    // where its answer is held for the slow entry's check.
    if verify::on() {
        word |= PROBE_CHECKED;
    }
    (*r).probe = word;
    word
}

/// This agent's coloured atoms by mask bit (the exact word a key list holds
/// for that key), written by [`seed_colours`] before user code runs.
#[thread_local]
static COLOUR_ATOMS: [std::cell::Cell<usize>; 127] = [const { std::cell::Cell::new(0) }; 127];

/// Colour this agent's atoms of the program's coloured keys (object model
/// step 5, case 2c). Runs once per agent with the static atom seeds, before
/// any key list is built from user code.
pub(crate) fn seed_colours() {
    verify::init();
    crate::object::layout_desc::family_diag_init();
    for (bytes, hash, bit, slot) in crate::object::layout_desc::coloured_atoms() {
        // SAFETY: the agent's atom table is live; `bytes` is image data.
        if let Some(a) = unsafe { crate::string::set_atom_colour(bytes, hash, bit, slot) } {
            COLOUR_ATOMS[bit as usize].set(a as usize);
        }
    }
}

/// Set record `rec`'s colour masks at its mint (slab insert): the record is
/// immutable from then on, so the masks are facts of the record like its key
/// list. One flags load per key position. An owned (unshared) list is
/// only ever changed in place by its one owner moving to a successor id, and
/// a tombstoned or not-yet-written slot holds `TAG_HOLE`, which every 2c arm
/// compares, so the masks never claim a slot that does not hold the key.
pub(crate) unsafe fn mint_colour_masks(rec: &mut ShapeRecord) {
    let keys = rec.keys as usize as *const crate::array::ArrayHeader;
    let (read, store) = if positional_layout(rec)
        && rec.logical_key_count != 0
        && !keys.is_null()
        && !crate::object::overflow::is_table(keys)
        && crate::value::addr_class::try_read_gc_header(keys as usize)
            .is_some_and(|gc| gc.obj_type == crate::gc::GC_TYPE_ARRAY)
    {
        colour_masks(rec)
    } else {
        ([0; 2], [0; 2])
    };
    rec.colour_mask = read;
    rec.colour_store_mask = store;
}

/// The coloured keys record `rec`'s receivers hold as inline data at their
/// colour slot (case 2c). A key's bit is set only when its position in the
/// record's immutable list IS its colour slot, the position is an inline
/// slot, the key is data (no accessor, not private), and no later position
/// repeats it.
unsafe fn colour_masks(rec: &ShapeRecord) -> ([u64; 2], [u64; 2]) {
    let mut mask = [0u64; 2];
    let mut store = [0u64; 2];
    let store_layout = matches!(
        rec.object_kind(),
        super::ShapeObjectKind::Ordinary | super::ShapeObjectKind::OrdinaryNativeAlias
    ) && rec.integrity_flags() == 0;
    let count = rec.logical_key_count as usize;
    let keys = rec.keys as usize as *const crate::array::ArrayHeader;
    let words = crate::array::array_elements_ptr(keys) as *const u64;
    // Only the list's own elements are read: a record's count is a prefix of
    // its list, so this bounds nothing for a published layout.
    let count = count.min((*keys).length as usize);
    let n = count.min(256);
    let inline = rec.live_inline_slot_count;
    let attrs = rec.summary()
        & (crate::object::key_attrs::SUMMARY_ACCESSOR
            | crate::object::key_attrs::SUMMARY_PRIVATE
            | crate::object::key_attrs::SUMMARY_NON_WRITABLE)
        != 0;
    for s in 0..n {
        let w = *words.add(s);
        if w & TAG_MASK != STRING_TAG {
            continue;
        }
        let a = (w & POINTER_MASK) as *const crate::StringHeader;
        if a.is_null() {
            continue;
        }
        let Some((bit, slot)) = crate::string::atom_colour(a) else {
            continue;
        };
        // The colour names this agent's atom of the key, stored at the
        // colour slot inline (the key's storage slot: its position unless
        // its layout keeps its own slots, #12637).
        let storage = crate::object::key_attrs::key_storage_slot(keys, s as u32);
        if storage != slot || storage >= inline || COLOUR_ATOMS[bit as usize].get() != a as usize {
            continue;
        }
        // A later position naming the same key owns the property: no bit.
        if (s + 1..count).any(|p| *words.add(p) == w) {
            continue;
        }
        let entry = if attrs {
            crate::object::key_attrs::keys_entry(keys, s as u32)
        } else {
            0
        };
        if entry
            & (crate::object::key_attrs::ENTRY_ACCESSOR_MASK
                | crate::object::key_attrs::ENTRY_PRIVATE)
            != 0
        {
            continue;
        }
        mask[(bit / 64) as usize] |= 1u64 << (bit % 64);
        if store_layout
            && entry & crate::object::key_attrs::ENTRY_NON_WRITABLE == 0
            && (storage >= crate::object::field_rep::REP_SLOTS
                || crate::object::field_rep::slot_rep(rec.rep, storage)
                    == crate::object::field_rep::REP_ANY)
        {
            store[(bit / 64) as usize] |= 1u64 << (bit % 64);
        }
    }
    (mask, store)
}

/// Does the record name its receivers' own properties by key position (key
/// position `i` is field `i`: inline below the live slot count, spilled at
/// or past it)? Every ordinary layout does: a descriptor or prototype
/// mutation mints a successor generation with the same key list and slots
/// (the generation exists for caches keyed on the id, which the probe has
/// none of), and a tombstone is a `TAG_HOLE` word no key equals. A native
/// namespace answers by position only at a complete generation, and a
/// per-object identity (`PROTO_ID_PER_OBJECT`) answers nothing.
fn positional_layout(rec: &ShapeRecord) -> bool {
    let kind = rec.object_kind();
    kind.is_ordinary_layout()
        && (kind != super::ShapeObjectKind::NativeNamespace
            || super::complete_layout_generation(rec.semantic_generation))
        && rec.proto_id != super::PROTO_ID_PER_OBJECT
}

/// Is `text` an array index (`0`, `1`, ...)? Element properties
/// (`ShapeRecord::element_kind`) live outside the key list, so the probe
/// never answers such a key; codegen never sends one either.
fn index_like(text: &[u8]) -> bool {
    !text.is_empty() && text.len() <= 10 && text.iter().all(u8::is_ascii_digit)
}

unsafe fn derive_word(rec: &ShapeRecord) -> u64 {
    let count = rec.logical_key_count;
    if !positional_layout(rec) {
        return PROBE_READY | PROBE_DECLINE;
    }
    let checked = if rec.summary()
        & (crate::object::key_attrs::SUMMARY_ACCESSOR | crate::object::key_attrs::SUMMARY_PRIVATE)
        != 0
    {
        PROBE_ATTRS
    } else {
        0
    };
    let keys = rec.keys as usize as *const crate::array::ArrayHeader;
    if count == 0 {
        return PROBE_READY | PROBE_ATOMS | checked;
    }
    if keys.is_null() || !super::keys_prefix_is_immutable(keys) {
        return PROBE_READY | PROBE_DECLINE;
    }
    let words = crate::array::array_elements_ptr(keys) as *const u64;
    // Canonical words and their hashes: an atom or symbol as it is; a short
    // or plain string as its text's program atom when one exists (a site key
    // is one), else as a TEXT entry indexed under its text's hash, which the
    // cold path compares by text (an atom of that text may be created later:
    // a computed key interns one). So every list of strings and symbols
    // proves absence.
    let mut atoms = true;
    let mut aliased = false;
    let mut canon: Vec<(u64, Option<u64>, bool)> = Vec::with_capacity(count as usize);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..count as usize {
        let bits = *words.add(i);
        if bits == TAG_HOLE {
            canon.push((bits, None, false));
            continue;
        }
        if let Some(h) = key_word_hash(bits) {
            canon.push((bits, Some(h), false));
            continue;
        }
        let v = crate::JSValue::from_bits(bits);
        let Some(text) = crate::string::js_string_key_bytes(v, &mut sso) else {
            atoms = false;
            canon.push((bits, None, false));
            continue;
        };
        let h = crate::object::key_bytes_hash(text.as_ptr(), text.len());
        aliased = true;
        match crate::string::atom_lookup(text, h) {
            Some(a) if crate::string::is_program_atom(a) => {
                canon.push((STRING_TAG | a as u64, Some(h), false))
            }
            // A runtime object whose keys were installed as plain strings
            // before the program's atoms existed (the global object, the
            // startup namespaces) may answer reads outside its list (a lazy
            // global): its TEXT entries answer hits only. An ordinary layout
            // and `%Object.prototype%` (whose unlisted names the walk knows)
            // prove absence by text.
            _ if !text_proves_absence(rec) => {
                atoms = false;
                canon.push((bits, Some(h), true))
            }
            _ => canon.push((bits, Some(h), true)),
        }
    }
    let flags = PROBE_READY | checked | if atoms { PROBE_ATOMS } else { 0 };
    // A list with accessor or private entries is always indexed: its
    // entries carry which positions a hit may read with one load.
    if count <= SCAN_KEYS && !aliased && checked == 0 {
        return flags;
    }
    let buckets = (count as usize * 2).next_power_of_two();
    // The blocking entries' hashes (`KeyIndex::blocks`), every one of them.
    let mut blocking: Vec<u64> = Vec::new();
    let mut unknown = false;
    if rec.summary()
        & (crate::object::key_attrs::SUMMARY_BLOCKS_STORE
            | crate::object::key_attrs::SUMMARY_PRIVATE)
        != 0
    {
        use crate::object::key_attrs::{ENTRY_ACCESSOR_MASK, ENTRY_NON_WRITABLE, ENTRY_PRIVATE};
        for (i, (w, h, _)) in canon.iter().enumerate() {
            if *w == TAG_HOLE
                || crate::object::key_attrs::keys_entry(keys, i as u32)
                    & (ENTRY_ACCESSOR_MASK | ENTRY_NON_WRITABLE | ENTRY_PRIVATE)
                    == 0
            {
                continue;
            }
            match h {
                Some(h) => blocking.push(*h),
                None => {
                    unknown = true;
                    blocking.clear();
                    break;
                }
            }
        }
    }
    let layout = index_layout(count as usize, buckets, blocking.len());
    let table = std::alloc::alloc(layout) as *mut KeyIndex;
    if table.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    (*table).mask = (buckets - 1) as u32;
    (*table).count = count;
    (*table)._pad = 0;
    (*table).nblocks = if unknown {
        BLOCKS_UNKNOWN
    } else {
        blocking.len() as u32
    };
    let block_out = KeyIndex::blocks(table) as *mut u64;
    for (i, h) in blocking.iter().enumerate() {
        *block_out.add(i) = *h;
    }
    let out = (*table).words.as_mut_ptr();
    for (i, (word, _, _)) in canon.iter().enumerate() {
        // An alias entry's word is its program atom; a TEXT entry's is its
        // text's hash, so a bucket compares the text only for a key of the
        // same hash.
        *out.add(i) = match canon[i] {
            (_, Some(h), true) => h,
            (w, _, _) if w != *words.add(i) => w,
            _ => 0,
        };
    }
    let slots = KeyIndex::buckets(table) as *mut u32;
    for b in 0..buckets {
        *slots.add(b) = u32::MAX;
    }
    for (i, (_, h, text)) in canon.iter().enumerate() {
        let Some(h) = h else {
            continue;
        };
        let mut b = *h as usize & (buckets - 1);
        while *slots.add(b) != u32::MAX {
            b = (b + 1) & (buckets - 1);
        }
        let alias = canon[i].0 != *words.add(i);
        *slots.add(b) = i as u32
            | if *text { TEXT_ENTRY } else { 0 }
            | if alias { ALIAS_ENTRY } else { 0 }
            | if direct_position(rec, keys, i as u32) {
                DIRECT_ENTRY
            } else {
                0
            };
    }
    table as u64 | flags
}

/// Does [`value_at`] read key position `pos` of `rec` (list `keys`) as the
/// inline slot at `pos`: a data key (no accessor, not private) whose storage
/// slot is its position, below the record's inline slot count?
unsafe fn direct_position(
    rec: &ShapeRecord,
    keys: *const crate::array::ArrayHeader,
    pos: u32,
) -> bool {
    use crate::object::key_attrs::{
        key_storage_slot, keys_entry, ENTRY_ACCESSOR_MASK, ENTRY_PRIVATE, SUMMARY_ACCESSOR,
        SUMMARY_PRIVATE,
    };
    if rec.summary() & (SUMMARY_ACCESSOR | SUMMARY_PRIVATE) != 0
        && keys_entry(keys, pos) & (ENTRY_ACCESSOR_MASK | ENTRY_PRIVATE) != 0
    {
        return false;
    }
    key_storage_slot(keys, pos) == pos && pos < rec.live_inline_slot_count
}

/// May a TEXT entry of `rec`'s list prove absence (see `derive_word`)?
unsafe fn text_proves_absence(rec: &ShapeRecord) -> bool {
    if rec.object_kind() == super::ShapeObjectKind::Ordinary {
        return true;
    }
    let dir = super::ordinary_dir_addr();
    match object_prototype(dir) {
        Some(p) => std::ptr::eq(record_of(dir, (*p).parent_class_id), rec),
        None => false,
    }
}

/// A TEXT entry's word (a short or plain string no program atom named when
/// the index was derived) against a site key: equal texts are one key.
#[cold]
#[inline(never)]
unsafe fn text_entry_matches(word: u64, key_bits: u64) -> bool {
    if key_bits & TAG_MASK != STRING_TAG {
        return false;
    }
    let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
    let key_text =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    crate::string::js_string_key_bytes(crate::JSValue::from_bits(word), &mut sso) == Some(key_text)
}

/// Probe bucket `pos` (an entry of a derived index) for `key_bits` (hash
/// `hash`): the key position on a match.
#[inline(always)]
unsafe fn bucket_matches(
    table: *const KeyIndex,
    list: *const u64,
    pos: u32,
    key_bits: u64,
    hash: u64,
) -> Option<u32> {
    let p = pos & POS_BITS;
    if pos & TEXT_ENTRY != 0 && KeyIndex::word(table, p) != hash {
        return None;
    }
    let w = if pos & ALIAS_ENTRY != 0 {
        KeyIndex::word(table, p)
    } else {
        *list.add(p as usize)
    };
    if w == key_bits || (pos & TEXT_ENTRY != 0 && text_entry_matches(w, key_bits)) {
        Some(p)
    } else {
        None
    }
}

/// The record ShapeId `word` names in the ordinary records at `dir`
/// (`BandDir`), or null.
#[inline(always)]
unsafe fn record_of(dir: *const u8, word: u32) -> *mut ShapeRecord {
    let base = *(dir.add(FLAT_BASE_OFFSET) as *const *mut ShapeRecord);
    let ids = *(dir.add(FLAT_BASE_OFFSET + 8) as *const usize);
    let rel =
        (word & !crate::object::overflow::SHADOW_BIT).wrapping_sub(super::SHAPE_ID_BASE) as usize;
    if rel >= ids {
        return std::ptr::null_mut();
    }
    base.add(rel)
}

/// The derived probe word of record `r`, or 0 when `r` is absent.
#[inline(always)]
unsafe fn probe_word(r: *mut ShapeRecord) -> u64 {
    let word = (*r).probe;
    if word != 0 {
        return word;
    }
    if !(*r).present() {
        return 0;
    }
    derive(r)
}

/// Own position of `key_bits` in record `r`'s list: `Ok(pos)`, `Err(true)`
/// when the list proves the key absent, `Err(false)` when it cannot answer.
#[inline(always)]
unsafe fn own_position(r: *mut ShapeRecord, key_bits: u64, hash: u64) -> Result<u32, bool> {
    let word = probe_word(r);
    if word == 0 || word & PROBE_DECLINE != 0 {
        return Err(false);
    }
    let count = (*r).logical_key_count;
    let absent = Err(word & PROBE_ATOMS != 0);
    if count == 0 {
        return absent;
    }
    let table = (word & !PROBE_FLAGS) as *const KeyIndex;
    if table.is_null() {
        let words = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
        let mut i = 0;
        while i < count {
            if *words.add(i as usize) == key_bits {
                return Ok(i);
            }
            i += 1;
        }
        return absent;
    }
    let list = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
    let mask = (*table).mask as usize;
    let slots = KeyIndex::buckets(table);
    let mut b = hash as usize & mask;
    loop {
        let pos = *slots.add(b);
        if pos == u32::MAX {
            return absent;
        }
        if let Some(p) = bucket_matches(table, list, pos, key_bits, hash) {
            return Ok(p);
        }
        b = (b + 1) & mask;
    }
}

/// The data value at key position `pos` of `obj` (record `r`), or `TAG_HOLE`
/// for an accessor, a hole, or storage the probe does not read.
#[cold]
#[inline(never)]
unsafe fn value_at(obj: *const ObjectHeader, r: *const ShapeRecord, pos: u32) -> f64 {
    let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
    if (*r).summary()
        & (crate::object::key_attrs::SUMMARY_ACCESSOR | crate::object::key_attrs::SUMMARY_PRIVATE)
        != 0
        && crate::object::key_attrs::keys_entry(keys, pos)
            & (crate::object::key_attrs::ENTRY_ACCESSOR_MASK
                | crate::object::key_attrs::ENTRY_PRIVATE)
            != 0
    {
        return hole();
    }
    // A key keeps its own storage slot when its layout separates key order
    // from storage (#12637); its position is its slot otherwise.
    let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
    if slot < (*r).live_inline_slot_count {
        return *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const f64);
    }
    crate::object::spill::spill_get_present(obj as usize, slot as usize)
        .map(f64::from_bits)
        .unwrap_or_else(hole)
}

/// The data value at key position `pos` of `obj` (record `r`): one load when
/// the record's facts make the position its inline slot (`direct`: the
/// entry's [`DIRECT_ENTRY`] bit, or `POSBOUND` for an unindexed list), else
/// [`value_at`]. A hole is the slot's value either way, which the site treats
/// as a decline.
#[inline(always)]
unsafe fn hit_value(
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
    pos: u32,
    direct: bool,
) -> f64 {
    if direct {
        return *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + pos as usize * 8)
            as *const f64);
    }
    value_at(obj, r, pos)
}

/// The value of key `key_bits` (atom hash `hash`) read from the object at
/// `obj_biased + RECEIVER_HANDLE_FLOOR`, or `TAG_HOLE` when the probe cannot
/// answer (the site's slow operation then does).
///
/// GC leaf: no allocation on the heap, no collection, no user code, no
/// thread-local access (`dir` is the agent's directory, which emitted code
/// reads from the agent pointer block).
///
/// # Safety
/// `dir` is this agent's ordinary directory or `PERRY_EMPTY_SHAPE_DIR`;
/// `obj_biased` a POINTER payload above the handle band, minus the floor;
/// `key_bits` a property atom (STRING-tagged) or a symbol; `hash` its hash.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    probe_get::<true>(dir, obj_biased, key_bits, hash)
}

/// [`perry_probe_get`] for a global environment binding's read, which must
/// tell an absent property from a stored `undefined` (HasBinding): it never
/// answers absence, only a property it finds.
///
/// # Safety
/// As [`perry_probe_get`].
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_binding(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    probe_get::<false>(dir, obj_biased, key_bits, hash)
}

/// [`perry_probe_get`] for a `length` site: an Array subclass instance
/// serves `length` from its elements store or its numeric tail, which its key
/// list does not describe alone, so it declines; every other ordinary
/// receiver's `length` is an ordinary key.
///
/// # Safety
/// As [`perry_probe_get`].
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_length(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let obj =
        (obj_biased as usize).wrapping_add(perry_abi::RECEIVER_HANDLE_FLOOR) as *const ObjectHeader;
    // Only an ordinary layout's receiver is an `ObjectHeader` to look into.
    if record_of(dir, (*obj).parent_class_id).is_null() {
        return hole();
    }
    let meta = (*obj).meta;
    let class_id = (*obj).class_id;
    if (!meta.is_null() && (*meta).elements != 0)
        || (class_id != 0 && crate::array::is_array_subclass_class_id(class_id))
    {
        return hole();
    }
    probe_get::<true>(dir, obj_biased, key_bits, hash)
}

#[inline(always)]
unsafe fn probe_get<const ABSENT: bool>(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    // The hit path keeps to caller-saved registers: every other outcome is a
    // tail call, so the leaf saves nothing on a hit.
    let obj =
        (obj_biased as usize).wrapping_add(perry_abi::RECEIVER_HANDLE_FLOOR) as *const ObjectHeader;
    let word = (*obj).parent_class_id;
    let r = record_of(dir, word);
    if r.is_null() {
        return array_named_get::<ABSENT>(dir, obj, key_bits, hash);
    }
    let probe = (*r).probe;
    if probe & (PROBE_READY | PROBE_DECLINE | PROBE_CHECKED) != PROBE_READY {
        return probe_get_underived::<ABSENT>(dir, obj, word, r, key_bits, hash);
    }
    let count = (*r).logical_key_count;
    if count == 0 {
        return absent_from::<ABSENT>(dir, obj, key_bits, hash);
    }
    let table = (probe & !PROBE_FLAGS) as *const KeyIndex;
    let (pos, direct) = if table.is_null() {
        // An unindexed list has no accessor or private entry.
        let words = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
        let mut i = 0u32;
        loop {
            if *words.add(i as usize) == key_bits {
                break (i, i < (*r).position_bound_raw());
            }
            i += 1;
            if i == count {
                return absent_from::<ABSENT>(dir, obj, key_bits, hash);
            }
        }
    } else {
        let list = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
        let mask = (*table).mask as usize;
        let slots = KeyIndex::buckets(table);
        let mut b = hash as usize & mask;
        loop {
            let pos = *slots.add(b);
            if pos == u32::MAX {
                return absent_from::<ABSENT>(dir, obj, key_bits, hash);
            }
            let i = pos & POS_BITS;
            if pos & TEXT_ENTRY != 0 {
                // Only a key of the text's hash can be its text.
                if KeyIndex::word(table, i) != hash {
                    b = (b + 1) & mask;
                    continue;
                }
                // A text compare is a call: the cold path makes it, so the
                // hit path keeps no frame.
                return probe_get_underived::<ABSENT>(dir, obj, word, r, key_bits, hash);
            }
            let w = if pos & ALIAS_ENTRY != 0 {
                KeyIndex::word(table, i)
            } else {
                *list.add(i as usize)
            };
            if w == key_bits {
                break (i, pos & DIRECT_ENTRY != 0);
            }
            b = (b + 1) & mask;
        }
    };
    let v = hit_value(obj, r, pos, direct);
    // A hole slot: the key is absent (the key view), and the answer
    // continues as for an absent key.
    if v.to_bits() != TAG_HOLE {
        return v;
    }
    own_hole::<ABSENT>(dir, obj, r, pos, key_bits, hash)
}

/// Is position `pos` of record `r` a plain data key (no accessor, no private
/// name)? `value_at`'s `TAG_HOLE` for such a key is a hole slot: the key is
/// absent.
#[inline]
unsafe fn plain_data_at(r: *const ShapeRecord, pos: u32) -> bool {
    (*r).summary()
        & (crate::object::key_attrs::SUMMARY_ACCESSOR | crate::object::key_attrs::SUMMARY_PRIVATE)
        == 0
        || crate::object::key_attrs::keys_entry(
            (*r).keys as usize as *const crate::array::ArrayHeader,
            pos,
        ) & (crate::object::key_attrs::ENTRY_ACCESSOR_MASK
            | crate::object::key_attrs::ENTRY_PRIVATE)
            == 0
}

/// The hit path read the hole at position `pos`: for a plain data key a
/// hole slot (the key is absent), else an accessor or storage the probe does
/// not read, which declines.
#[cold]
#[inline(never)]
unsafe fn own_hole<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    pos: u32,
    key_bits: u64,
    hash: u64,
) -> f64 {
    if !plain_data_at(r, pos) {
        return hole();
    }
    own_absent::<ABSENT>(dir, obj, r, key_bits, hash)
}

/// The receiver's own key at position `pos`: its value, or, for a hole slot
/// of a plain data key, the absent key's answer.
#[cold]
#[inline(never)]
unsafe fn own_value<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    pos: u32,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let v = value_at(obj, r, pos);
    if v.to_bits() != TAG_HOLE || !plain_data_at(r, pos) {
        return v;
    }
    own_absent::<ABSENT>(dir, obj, r, key_bits, hash)
}

/// A key the receiver's layout lists but whose slot holds the hole: absent
/// from the slots, so the overflow (a re-add) and then the prototype chain
/// answer, exactly as for a key the layout does not list.
#[cold]
#[inline(never)]
unsafe fn own_absent<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> f64 {
    if (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT != 0 {
        if let Some(v) = overflow_get(obj, key_bits, hash) {
            return v;
        }
    }
    inherited_get::<ABSENT>(dir, obj, r, key_bits, hash)
}

/// A receiver whose header word names no ordinary layout. Two exotics keep
/// their own named properties in one ordinary object: an Array's
/// `ArrayHeader.named` (object model step 7) and a function's own-property
/// bag (`ClosureHeader.props`, `closure/props.rs`: the function's own string
/// and symbol properties, `prototype` once it exists; a deletion removes the
/// key from the bag). The probe asks that object for an own data property and
/// on a proven miss continues at the exotic's builtin prototype, a static
/// layout (`runtime_layouts::builtin_prototypes`):
///
/// * an array with no own [[Prototype]] (`GC_ARRAY_CUSTOM_PROTO`) inherits
///   from `%Array.prototype%`, whose holder lists every builtin method, then
///   `%Object.prototype%`;
/// * a function on a described Function shape (`closure::shape`: no accessor,
///   delete or recorded [[Prototype]]) of a plain body: its own keys are its
///   layout's intrinsic `length`, `name` and (a constructor's) `prototype`
///   ([`function_own_intrinsic`]) and its bag; then it inherits from
///   `%Function.prototype%`.
///
/// Everything else declines.
#[cold]
#[inline(never)]
unsafe fn array_named_get<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let v = exotic_get::<ABSENT>(dir, obj, key_bits, hash);
    if ABSENT {
        verify::answer(obj as usize, key_bits, v)
    } else {
        v
    }
}

#[inline(always)]
unsafe fn exotic_get<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return hole();
    }
    let (named, proto, function) = match (*header).obj_type {
        crate::gc::GC_TYPE_ARRAY => {
            // Array length is an intrinsic own property, stored in the array
            // header rather than its named bag. Never prove it absent by
            // walking the bag and prototypes (including for computed keys
            // read by a Proxy's get trap).
            if hash == crate::object::runtime_layouts::atom_hash(b"length") && {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                crate::string::js_string_key_bytes(
                    crate::JSValue::from_bits(key_bits),
                    &mut scratch,
                ) == Some(b"length".as_slice())
            } {
                return (*(obj as *const crate::array::ArrayHeader)).length as f64;
            }
            let named = (*(obj as *const crate::array::ArrayHeader)).named as *const ObjectHeader;
            if (*header)._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
                (named, None, false)
            } else {
                (
                    named,
                    Some(crate::closure::shape::INTRINSIC_SERIAL_ARRAY_PROTOTYPE),
                    false,
                )
            }
        }
        crate::gc::GC_TYPE_CLOSURE => {
            let c = obj as *const crate::closure::ClosureHeader;
            let plain = crate::closure::shape::closure_on_plain_function_shape(c);
            (
                (*c).props as *const ObjectHeader,
                plain.then_some(crate::closure::shape::INTRINSIC_SERIAL_FUNCTION),
                plain,
            )
        }
        _ => return hole(),
    };
    if !named.is_null() {
        let word = (*named).parent_class_id;
        let r = record_of(dir, word);
        if r.is_null() {
            return hole();
        }
        match own_position(r, key_bits, hash) {
            Ok(pos) => return value_at(named, r, pos),
            Err(true) if word & crate::object::overflow::SHADOW_BIT != 0 => {
                match overflow_get(named, key_bits, hash) {
                    Some(v) => return v,
                    None => {}
                }
            }
            Err(true) => {}
            Err(false) => return hole(),
        }
    }
    let Some(serial) = proto else {
        return hole();
    };
    if function {
        // Not in the bag: the function layout's intrinsic own keys.
        if let Some(v) = function_own_intrinsic(obj.cast(), key_bits, hash) {
            return v;
        }
    }
    let Some(p) = intrinsic_prototype(dir, serial) else {
        return hole();
    };
    if serial == crate::closure::shape::INTRINSIC_SERIAL_ARRAY_PROTOTYPE {
        // `p` is `%Array.prototype%`'s holder; `%Array.prototype%` itself
        // (whose holder `named` is) continues at its own [[Prototype]]. A
        // write to the holder, or a relink of `%Array.prototype%`, sets the
        // holder's shadow bit.
        if p != named {
            let w = (*p).parent_class_id;
            let rn = record_of(dir, w);
            if rn.is_null() || w & crate::object::overflow::SHADOW_BIT != 0 {
                return hole();
            }
            let pw = probe_word(rn);
            if pw & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS {
                return hole();
            }
            match list_position(rn, pw, key_bits, hash) {
                Ok((pos, direct)) => return hit_value(p, rn, pos, direct),
                Err(true) => {}
                Err(false) => return hole(),
            }
        }
        // `%Array.prototype%`'s [[Prototype]] is `%Object.prototype%`.
        return match object_prototype(dir) {
            Some(op) => exotic_chain::<ABSENT>(dir, op, key_bits, hash),
            None => hole(),
        };
    }
    exotic_chain::<ABSENT>(dir, p, key_bits, hash)
}

/// [`chain_from`] from an exotic receiver's builtin prototype `next`: there
/// is no ordinary object to resume a complete walk from.
#[inline(never)]
unsafe fn exotic_chain<const ABSENT: bool>(
    dir: *const u8,
    next: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    chain_from::<ABSENT>(
        dir,
        std::ptr::null(),
        std::ptr::null(),
        next,
        key_bits,
        hash,
    )
}

/// A plain function's own intrinsic keys, which its layout lists before its
/// bag's (ECMA-262 §10.2.9 SetFunctionLength / §10.2.8 SetFunctionName,
/// §10.2.5 MakeConstructor): `length` and `name`, static data of its body,
/// and a constructor's `prototype`. `None` for any other key (it continues at
/// `%Function.prototype%`); otherwise the own value, or the hole marker when
/// the value is not static data: a name that is not an image string, or a
/// constructor's `prototype`, created on its first read. A bag entry (a
/// redefinition, a materialized `prototype`) was answered before this.
///
/// # Safety
/// `c` is a closure on a described Function shape of a plain body.
#[inline(always)]
unsafe fn function_own_intrinsic(
    c: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    use crate::object::runtime_layouts::atom_hash as h;
    const NAME: u64 = h(b"name");
    const LENGTH: u64 = h(b"length");
    const PROTOTYPE: u64 = h(b"prototype");
    if !matches!(hash, NAME | LENGTH | PROTOTYPE) {
        return None;
    }
    function_own_intrinsic_keyed(c, key_bits, hash)
}

#[inline(never)]
unsafe fn function_own_intrinsic_keyed(
    c: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    use crate::object::runtime_layouts::atom_hash as h;
    let text: &[u8] = match hash {
        x if x == h(b"name") => b"name",
        x if x == h(b"length") => b"length",
        _ => b"prototype",
    };
    // A hash equal to an intrinsic's is that key only if the texts agree.
    if key_bits & TAG_MASK != STRING_TAG {
        return Some(hole());
    }
    let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
    if (*key).byte_len as usize != text.len()
        || std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            text.len(),
        ) != text
    {
        return None;
    }
    let info = (*c).info;
    Some(match text {
        b"length" => crate::closure::info_length(&*info)
            .or_else(|| crate::closure::info_arity(&*info))
            .unwrap_or(0) as f64,
        b"name" => match crate::builtins::function_name_image(info) {
            Some(s) => f64::from_bits(STRING_TAG | s as u64),
            None => hole(),
        },
        _ => {
            // Arrows and non-constructors have no own `prototype`.
            let header = crate::gc::header_from_trusted_user_ptr(c.cast());
            if (*info).flags & (crate::closure::FN_ARROW | crate::closure::FN_NON_CONSTRUCTOR) != 0
                || (*header)._reserved & crate::gc::GC_CLOSURE_NON_CONSTRUCTABLE != 0
            {
                return None;
            }
            hole()
        }
    })
}

/// The builtin prototype object a reserved intrinsic `serial` names, read
/// from its identity word in the agent's word directory (the encoding of
/// runtime prototype families, `layout_desc::PerryProtoFamily`): `None`
/// before the realm named it.
#[inline(always)]
unsafe fn intrinsic_prototype(dir: *const u8, serial: u64) -> Option<*const ObjectHeader> {
    let word_dir = *(dir.add(FLAT_BASE_OFFSET + 24) as *const *const u8);
    if word_dir.is_null() {
        return None;
    }
    let bits = *super::shapes_prototype::identity_word_slot_in(word_dir, serial)?;
    if bits & TAG_MASK != POINTER_TAG
        || ((bits & POINTER_MASK) as usize) < perry_abi::RECEIVER_HANDLE_FLOOR
    {
        return None;
    }
    Some((bits & POINTER_MASK) as *const ObjectHeader)
}

/// The record's probe word is not derived yet, or declines.
#[cold]
#[inline(never)]
unsafe fn probe_get_underived<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    word: u32,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let v = match own_position(r, key_bits, hash) {
        Ok(pos) => own_value::<ABSENT>(dir, obj, r, pos, key_bits, hash),
        Err(proven) => {
            let probe = if proven { (*r).probe } else { 0 };
            probe_get_absent::<ABSENT>(dir, obj, word, r, probe, key_bits, hash)
        }
    };
    if ABSENT {
        verify::answer(obj as usize, key_bits, v)
    } else {
        v
    }
}

/// The receiver's layout does not hold the key: its overflow, then its
/// prototype chain. `probe` is the receiver's probe word when its list
/// proved the key absent (`PROBE_ATOMS`), else 0.
#[inline(always)]
unsafe fn probe_get_absent<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    word: u32,
    r: *mut ShapeRecord,
    probe: u64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    // Every outcome is a tail call or a constant, so the caller's hit path
    // keeps its frameless form.
    if word & crate::object::overflow::SHADOW_BIT != 0 {
        return overflow_then_inherited::<ABSENT>(dir, obj, r, probe, key_bits, hash);
    }
    if probe & PROBE_ATOMS == 0 {
        return hole();
    }
    inherited_get::<ABSENT>(dir, obj, r, key_bits, hash)
}

/// [`probe_get_absent`] for a receiver with an overflow.
#[inline(never)]
unsafe fn overflow_then_inherited<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    probe: u64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    match overflow_get(obj, key_bits, hash) {
        Some(v) => v,
        None if probe & PROBE_ATOMS == 0 => hole(),
        None => inherited_get::<ABSENT>(dir, obj, r, key_bits, hash),
    }
}

/// The overflow entry for `key_bits`: `Some(value)` (`TAG_HOLE` for an
/// accessor, which the slow operation calls), `None` when the overflow
/// proves the key absent.
unsafe fn overflow_get(obj: *const ObjectHeader, key_bits: u64, hash: u64) -> Option<f64> {
    let meta = (*obj).meta;
    // A shadow bit with no overflow (an intrinsic prototype written in
    // place, a relink, a deleted shadow name): the layout's list is every
    // own key.
    if meta.is_null() || (*meta).overflow == 0 {
        return None;
    }
    let t = (*meta).overflow as usize as *const crate::object::overflow::Table;
    let i = crate::object::overflow::find_hashed(t, key_bits, hash)?;
    if *crate::object::overflow::attrs_at(t, i) as u8
        & crate::object::key_attrs::ENTRY_ACCESSOR_MASK
        != 0
    {
        return Some(hole());
    }
    Some(
        crate::object::overflow::get_from(t, obj, i)
            .map(f64::from_bits)
            .unwrap_or_else(hole),
    )
}

/// May the probe continue from `obj` (record `r`) to its prototype? Only an
/// object whose own keys are exactly its layout's list and its overflow: an
/// ordinary object, or an unmarked or callable one that [`listed`] admits; no
/// native-alias, namespace or numeric-proof kind, no exotic read receiver.
#[inline(always)]
unsafe fn walks_from(obj: *const ObjectHeader, r: *const ShapeRecord) -> bool {
    match (*r).object_kind() {
        super::ShapeObjectKind::Ordinary => {
            let meta = (*obj).meta;
            meta.is_null()
                || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER == 0
        }
        kind => listed(obj, kind),
    }
}

/// Are the own keys of `obj`, an object of the unmarked or callable ordinary
/// `kind`, exactly its list and overflow? An unmarked object qualifies (a
/// runtime-born record, a user prototype, a sealed intrinsic of any family:
/// the builtin and runtime prototypes born on complete static layouts) unless
/// it is an exotic read receiver or a typed-array prototype; a callable one
/// only as such a prototype on its static layout (`%Function.prototype%`,
/// [`crate::object::builtin_proto::on_static_layout`]).
#[inline(always)]
unsafe fn listed(obj: *const ObjectHeader, kind: super::ShapeObjectKind) -> bool {
    let meta = (*obj).meta;
    let flags = if meta.is_null() { 0 } else { (*meta).flags };
    if flags & crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0 {
        return flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER == 0
            && match kind {
                super::ShapeObjectKind::OrdinaryUnmarked => true,
                super::ShapeObjectKind::CallableOrdinary => {
                    crate::object::builtin_proto::on_static_layout(flags)
                }
                _ => false,
            };
    }
    if kind != super::ShapeObjectKind::OrdinaryUnmarked
        || flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0
    {
        return false;
    }
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    (*header)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO == 0
}

/// Where a prototype walk goes from an object.
enum Next {
    /// The [[Prototype]] object.
    Object(*const ObjectHeader),
    /// The chain ends here ([[Prototype]] is null).
    End,
    /// A link the probe does not follow.
    Unknown,
}

/// The agent's `Object.prototype`, read through its shape directory `dir`
/// (`BandDir.proto_cell`): `None` before it is resolved.
#[inline]
unsafe fn object_prototype(dir: *const u8) -> Option<*const ObjectHeader> {
    let cell = *(dir.add(FLAT_BASE_OFFSET + 16) as *const *const usize);
    if cell.is_null() {
        return None;
    }
    let p = *cell;
    if p == usize::MAX || p < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    // The cell names the realm's `%Object.prototype%`, an ordinary heap
    // object, so its header is read directly. The collector rewrites the
    // cell; a cell naming a forwarded copy is not followed (the slow path
    // heals it).
    let h = crate::gc::header_from_trusted_user_ptr(p as *const u8);
    ((*h).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0).then_some(p as *const ObjectHeader)
}

/// `obj`'s [[Prototype]] (`r` is its record), by the rule the runtime's own
/// shape link reads (`object_ops::prototype::shape_link_prototype` and
/// `shapes::object_prototype_word`): a recorded prototype first, then the
/// shape's identity: null, the realm default `%Object.prototype%`, a class
/// declaration prototype (the identity's word), or a linked identity's word.
unsafe fn prototype_of(dir: *const u8, obj: *const ObjectHeader, r: *const ShapeRecord) -> Next {
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*header)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        && ((*obj).meta.is_null() || (*(*obj).meta).prototype == 0)
        && (*r).proto_id == super::PROTO_ID_DEFAULT
    {
        return Next::Unknown;
    }
    link_of(dir, obj, r)
}

/// The [[Prototype]] link of `cur` (record `rc`), an object whose own keys
/// [`walks_from`] admits, so not a typed-array prototype: its recorded
/// prototype, else its layout's identity (null, the realm default
/// `%Object.prototype%`, a class identity's word, a linked identity's word).
#[inline(always)]
unsafe fn link_of(dir: *const u8, cur: *const ObjectHeader, rc: *const ShapeRecord) -> Next {
    let meta = (*cur).meta;
    let mut bits = 0u64;
    if !meta.is_null() {
        if (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0 {
            return Next::Unknown;
        }
        bits = (*meta).prototype;
    }
    if bits == 0 {
        if (*rc).weak_collection_brand().is_some() {
            return Next::Unknown;
        }
        let pid = (*rc).proto_id;
        if pid == super::PROTO_ID_DEFAULT {
            return match object_prototype(dir) {
                Some(p) if p != cur => Next::Object(p),
                _ => Next::Unknown,
            };
        }
        if pid == super::PROTO_ID_NULL {
            return Next::End;
        }
        bits = if (super::PROTO_ID_CLASS..super::PROTO_ID_MIXED).contains(&pid) {
            let word_dir = *(dir.add(FLAT_BASE_OFFSET + 24) as *const *const u8);
            if word_dir.is_null() {
                return Next::Unknown;
            }
            match super::shapes_prototype::identity_word_slot_in(word_dir, pid) {
                Some(slot) => *slot,
                None => return Next::Unknown,
            }
        } else {
            (*rc).prototype_word()
        };
    }
    if bits == TAG_NULL {
        return Next::End;
    }
    if bits & TAG_MASK != POINTER_TAG
        || ((bits & POINTER_MASK) as usize) < perry_abi::RECEIVER_HANDLE_FLOOR
    {
        return Next::Unknown;
    }
    Next::Object((bits & POINTER_MASK) as *const ObjectHeader)
}

/// The first prototype of `obj` that holds `key_bits`, if every object
/// before it proves the key absent, and that holder's data value; with
/// `ABSENT`, `undefined` when the whole chain proves the key absent.
///
/// `obj`'s own list and overflow have proved the key absent; the chain is
/// read by [`chain_from`].
#[inline(never)]
unsafe fn inherited_get<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> f64 {
    // `cur` is an object the walk may continue from (`walks_from`): the
    // receiver is checked here, each prototype before it becomes `cur`.
    if (*r).object_kind() != super::ShapeObjectKind::Ordinary {
        return inherited_walk::<ABSENT>(dir, obj, r, key_bits, hash);
    }
    match link_of(dir, obj, r) {
        Next::Object(next) => chain_from::<ABSENT>(dir, obj, r, next, key_bits, hash),
        Next::End => chain_absent::<ABSENT>(),
        Next::Unknown => hole(),
    }
}

/// The chain from `next`, the [[Prototype]] of `cur` (record `rc`, an object
/// whose own list and overflow proved `key_bits` absent; null for an exotic
/// receiver), read in one frame-light loop with each hop's work the layout's
/// own facts and the object's own words:
///
/// * the object's layout word, record and key index, the same probe the own
///   hit makes; a hit reads the holder's slot (`hit_value`);
/// * a miss continues while the object's own keys are exactly its list (no
///   shadow bit; [`walks_from`]), through its link ([`link_of`]), and ends
///   where a link is null. The builtin prototypes are static layouts
///   (`runtime_layouts::builtin_prototypes`), so `%Object.prototype%`'s miss
///   is absence like any other.
///
/// Every other case (an overflow on a prototype, a TEXT key entry, a record
/// not yet derived) continues in the complete walk ([`inherited_walk`]) from
/// `cur`, which answers it with the same rules; with no `cur` it declines.
#[inline(always)]
unsafe fn chain_from<const ABSENT: bool>(
    dir: *const u8,
    mut cur: *const ObjectHeader,
    mut rc: *const ShapeRecord,
    mut next: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let walk = |cur: *const ObjectHeader, rc: *const ShapeRecord| {
        if cur.is_null() {
            hole()
        } else {
            inherited_walk::<ABSENT>(dir, cur, rc as *mut _, key_bits, hash)
        }
    };
    for _ in 0..MAX_HOPS {
        let w = (*next).parent_class_id;
        let rn = record_of(dir, w);
        if rn.is_null() {
            return hole();
        }
        // A record's probe word is derived on its first probe.
        let pw = probe_word(rn);
        if pw & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS {
            return walk(cur, rc);
        }
        match list_position(rn, pw, key_bits, hash) {
            Ok((pos, direct)) => {
                let v = hit_value(next, rn, pos, direct);
                if v.to_bits() != TAG_HOLE {
                    return v;
                }
                // A hole slot is absent on this hop: the complete walk
                // continues past it.
                return walk(cur, rc);
            }
            Err(true) => {}
            Err(false) => return walk(cur, rc),
        }
        if w & crate::object::overflow::SHADOW_BIT != 0 {
            return walk(cur, rc);
        }
        // A sealed builtin prototype not yet on a static layout may install
        // methods outside its list, so its miss proves nothing.
        match (*rn).object_kind() {
            super::ShapeObjectKind::Ordinary => {}
            kind if listed(next, kind) => {}
            _ => return hole(),
        }
        match link_of(dir, next, rn) {
            Next::Object(p) => {
                cur = next;
                rc = rn;
                next = p;
            }
            Next::End => return chain_absent::<ABSENT>(),
            Next::Unknown => return hole(),
        }
    }
    hole()
}

/// The chain proved the key absent.
#[inline(always)]
fn chain_absent<const ABSENT: bool>() -> f64 {
    if ABSENT {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        hole()
    }
}

/// Position of `key_bits` in record `r`'s derived list (`pw` its ready probe
/// word with `PROBE_ATOMS`) and whether a hit there is one load
/// ([`hit_value`]): `Ok((pos, direct))`, `Err(true)` for proven absence,
/// `Err(false)` for a TEXT entry the complete walk compares by text.
#[inline(always)]
unsafe fn list_position(
    r: *const ShapeRecord,
    pw: u64,
    key_bits: u64,
    hash: u64,
) -> Result<(u32, bool), bool> {
    let count = (*r).logical_key_count;
    if count == 0 {
        return Err(true);
    }
    let list = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
    let table = (pw & !PROBE_FLAGS) as *const KeyIndex;
    if table.is_null() {
        let mut i = 0;
        while i < count {
            if *list.add(i as usize) == key_bits {
                return Ok((i, i < (*r).position_bound_raw()));
            }
            i += 1;
        }
        return Err(true);
    }
    let mask = (*table).mask as usize;
    let slots = KeyIndex::buckets(table);
    let mut b = hash as usize & mask;
    loop {
        let pos = *slots.add(b);
        if pos == u32::MAX {
            return Err(true);
        }
        let i = pos & POS_BITS;
        if pos & TEXT_ENTRY != 0 {
            // Only a key of the text's hash can be its text, which the
            // complete walk compares.
            if KeyIndex::word(table, i) != hash {
                b = (b + 1) & mask;
                continue;
            }
            return Err(false);
        }
        let wd = if pos & ALIAS_ENTRY != 0 {
            KeyIndex::word(table, i)
        } else {
            *list.add(i as usize)
        };
        if wd == key_bits {
            return Ok((i, pos & DIRECT_ENTRY != 0));
        }
        b = (b + 1) & mask;
    }
}

/// The hit path's proven own miss (its list proved the key absent): the
/// overflow, then the prototype chain. The record is read again from the
/// object, so the hit path passes four values and keeps no frame.
#[inline(never)]
unsafe fn absent_from<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let word = (*obj).parent_class_id;
    let r = record_of(dir, word);
    let probe = (*r).probe;
    if word & crate::object::overflow::SHADOW_BIT != 0 {
        return overflow_then_inherited::<ABSENT>(dir, obj, r, probe, key_bits, hash);
    }
    // A list with a key no compare can match (`PROBE_ATOMS` clear) proves
    // nothing by a miss.
    if probe & PROBE_ATOMS == 0 {
        return hole();
    }
    inherited_get::<ABSENT>(dir, obj, r, key_bits, hash)
}

/// The complete prototype walk of [`inherited_get`].
#[inline(never)]
unsafe fn inherited_walk<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let absent = || {
        if ABSENT {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            hole()
        }
    };
    let (mut cur, mut rc) = (obj, r as *const ShapeRecord);
    for _ in 0..MAX_HOPS {
        if !walks_from(cur, rc) {
            // A runtime-born record (unmarked, its keys all in its list and
            // overflow) whose chain ends here proves absence.
            if (*rc).object_kind() == super::ShapeObjectKind::OrdinaryUnmarked
                && ((*cur).meta.is_null()
                    || (*(*cur).meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                        == 0)
                && matches!(prototype_of(dir, cur, rc), Next::End)
            {
                return absent();
            }
            return hole();
        }
        let next = match prototype_of(dir, cur, rc) {
            Next::Object(p) => p,
            Next::End => return absent(),
            Next::Unknown => return hole(),
        };
        let w = (*next).parent_class_id;
        let rn = record_of(dir, w);
        if rn.is_null() {
            return hole();
        }
        match own_position(rn, key_bits, hash) {
            Ok(pos) => {
                let v = value_at(next, rn, pos);
                // A hole slot of a plain data key: absent on this hop.
                if v.to_bits() != TAG_HOLE || !plain_data_at(rn, pos) {
                    return v;
                }
            }
            Err(false) => return hole(),
            Err(true) => {}
        }
        if w & crate::object::overflow::SHADOW_BIT != 0 {
            if let Some(v) = overflow_get(next, key_bits, hash) {
                return v;
            }
        }
        // A sealed builtin prototype not yet on a static layout may install
        // methods outside its list, so its miss proves nothing.
        match (*rn).object_kind() {
            super::ShapeObjectKind::Ordinary => {}
            kind if listed(next, kind) => {}
            _ => return hole(),
        }
        cur = next;
        rc = rn;
    }
    hole()
}

/// The accessor `[[Get]]` of `key` on `obj` would call, when the probe's
/// walk finds the key as an accessor of `obj` or of the first prototype that
/// holds it (every object before it proving the key absent). `None` for
/// everything else, which the caller's complete operation answers. Not a
/// leaf contract of its own: it reads exactly what [`perry_probe_get`] reads.
///
/// # Safety
/// `obj` is a live object above the handle band; `key` a live string.
pub(crate) unsafe fn probe_accessor(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<crate::object::accessor_pair::Accessor> {
    if key.is_null() || (obj as usize) < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let atom = if crate::string::is_property_atom(key) {
        key
    } else {
        crate::string::existing_property_atom(key)?
    };
    probe_accessor_word(
        obj,
        STRING_TAG | atom as u64,
        crate::string::property_atom_hash(atom),
    )
}

/// [`probe_accessor`] for a key word as a site passes it: a property atom
/// (STRING-tagged) or a symbol, and its hash.
///
/// # Safety
/// `obj` is a live object above the handle band.
pub(crate) unsafe fn probe_accessor_word(
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> Option<crate::object::accessor_pair::Accessor> {
    if (obj as usize) < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let dir = super::ordinary_dir_addr();
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let mut cur = obj;
    let mut rc = record_of(dir, (*cur).parent_class_id) as *const ShapeRecord;
    if rc.is_null() || !(*rc).present() {
        return None;
    }
    for hop in 0..=MAX_HOPS {
        if hop > 0
            || (*rc).object_kind() == super::ShapeObjectKind::Ordinary
            || listed(cur, (*rc).object_kind())
        {
            match own_position(rc as *mut ShapeRecord, key_bits, hash) {
                Ok(pos) => return accessor_at(cur, rc, pos),
                Err(false) => return None,
                Err(true) => {}
            }
            if (*cur).parent_class_id & crate::object::overflow::SHADOW_BIT != 0
                && !matches!(overflow_get(cur, key_bits, hash), None)
            {
                return None;
            }
        } else {
            return None;
        }
        if !walks_from(cur, rc) {
            return None;
        }
        let next = match prototype_of(dir, cur, rc) {
            Next::Object(p) => p,
            _ => return None,
        };
        let rn = record_of(dir, (*next).parent_class_id) as *const ShapeRecord;
        if rn.is_null() || !(*rn).present() {
            return None;
        }
        match (*rn).object_kind() {
            super::ShapeObjectKind::Ordinary => {}
            kind if listed(next, kind) => {}
            _ => {
                // A builtin prototype's list holds what the program defined
                // on it; an accessor found there is that accessor.
                return match own_position(rn as *mut ShapeRecord, key_bits, hash) {
                    Ok(pos) => accessor_at(next, rn, pos),
                    Err(_) => None,
                };
            }
        }
        cur = next;
        rc = rn;
    }
    None
}

/// The accessor at key position `pos` of `obj` (record `r`), if that key is
/// an accessor.
unsafe fn accessor_at(
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
    pos: u32,
) -> Option<crate::object::accessor_pair::Accessor> {
    if !positional_layout(&*r) || (*r).summary() & crate::object::key_attrs::SUMMARY_ACCESSOR == 0 {
        return None;
    }
    let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
    if crate::object::key_attrs::keys_entry(keys, pos)
        & crate::object::key_attrs::ENTRY_ACCESSOR_MASK
        == 0
    {
        return None;
    }
    let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
    let bits = if slot < (*r).live_inline_slot_count {
        *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64)
    } else {
        crate::object::spill::spill_get_present(obj as usize, slot as usize)?
    };
    Some(crate::object::accessor_pair::pair_of_value(bits).unwrap_or_default())
}

/// The value of `obj`'s own PRIVATE entry `key_bits` (a property atom,
/// `hash` its hash), read by its layout as the probe reads a public key;
/// `None` when the layout does not list it (or cannot answer). A runtime
/// intrinsic's private field is the only reader: a public read never sees a
/// private entry (`value_at`).
///
/// # Safety
/// `obj` is a live object above the handle band.
pub(crate) unsafe fn own_private_value(
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    let dir = super::ordinary_dir_addr();
    let r = record_of(dir, (*obj).parent_class_id);
    if r.is_null() || !(*r).present() || !positional_layout(&*r) {
        return None;
    }
    let pos = own_position(r, key_bits, hash).ok()?;
    let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
    if crate::object::key_attrs::keys_entry(keys, pos) & crate::object::key_attrs::ENTRY_PRIVATE
        == 0
    {
        return None;
    }
    let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
    let bits = if slot < (*r).live_inline_slot_count {
        *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64)
    } else {
        crate::object::spill::spill_get_present(obj as usize, slot as usize)?
    };
    (bits != TAG_HOLE).then_some(f64::from_bits(bits))
}

/// A class method site's own-override check (`this.m = fn` shadowing the
/// class method, perry-codegen `emit_own_method_override_check`): the own
/// DATA value of `key_bits` (the site's property atom, `hash` its hash) on
/// `recv`, or `undefined` when `recv` has no own data property of that name
/// (absent, a hole, an accessor). The receiver's layout answers it with the
/// probe's own lookup (one index probe, atoms compared by identity); what the
/// layout cannot answer takes the by-bytes own scan
/// (`js_object_get_own_field_or_undef`), which this answers identically.
///
/// GC leaf, like the by-bytes scan.
///
/// # Safety
/// `dir` is this agent's ordinary directory or `PERRY_EMPTY_SHAPE_DIR`;
/// `key_bits` a STRING-tagged property atom; `recv` any value.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_own_method(
    dir: *const u8,
    recv: f64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let bits = recv.to_bits();
    let payload = (bits & POINTER_MASK) as usize;
    if bits & TAG_MASK == POINTER_TAG && payload >= perry_abi::RECEIVER_HANDLE_FLOOR {
        let h = crate::gc::header_from_trusted_user_ptr(payload as *const u8);
        let obj = payload as *const ObjectHeader;
        if (*h).obj_type == crate::gc::GC_TYPE_OBJECT
            && (*h).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT == 0
        {
            let r = record_of(dir, (*obj).parent_class_id);
            if !r.is_null() && (*r).present() {
                match own_position(r, key_bits, hash) {
                    Ok(pos) => {
                        let v = value_at(obj, r, pos);
                        return if v.to_bits() == TAG_HOLE {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        } else {
                            v
                        };
                    }
                    Err(true) => return f64::from_bits(crate::value::TAG_UNDEFINED),
                    Err(false) => {}
                }
            }
        }
    }
    own_method_by_bytes(recv, key_bits)
}

/// [`perry_probe_own_method`]'s by-bytes scan of the key's text.
#[cold]
#[inline(never)]
unsafe fn own_method_by_bytes(recv: f64, key_bits: u64) -> f64 {
    let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
    if key_bits & TAG_MASK != STRING_TAG || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::object::js_object_get_own_field_or_undef(
        recv,
        crate::string::string_data(key),
        (*key).byte_len as usize,
    )
}

#[path = "layout_probe_store.rs"]
mod store;
use store::class_prototype_object;
pub(crate) use store::{layouts_admit_add, perry_probe_get_value, perry_probe_set, probe_has};

#[path = "layout_probe_computed.rs"]
mod computed;
use computed::computed_key;
pub use computed::{perry_probe_get_keyed, perry_probe_set_keyed};

/// This agent's ordinary shape directory (tests).
#[cfg(test)]
pub(crate) fn test_dir() -> *const u8 {
    super::ordinary_dir_addr()
}

/// Key position `pos` of `obj`'s record's list (tests).
#[cfg(test)]
pub(crate) unsafe fn test_list_word(obj: *const ObjectHeader, pos: usize) -> u64 {
    let r = record_of(super::ordinary_dir_addr(), (*obj).parent_class_id);
    assert!(!r.is_null(), "the object has an ordinary record");
    *(crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64).add(pos)
}

#[path = "layout_probe_diag.rs"]
mod diagnostic;
pub(crate) use diagnostic::{diag_enabled, diag_explain, verify};

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_PROBE_GET_VALUE: unsafe extern "C" fn(*const u8, u64, u64, u64) -> f64 =
    perry_probe_get_value;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_PROBE_GET: unsafe extern "C" fn(*const u8, i64, u64, u64) -> f64 =
    perry_probe_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_PROBE_GET_BINDING: unsafe extern "C" fn(*const u8, i64, u64, u64) -> f64 =
    perry_probe_get_binding;

#[cfg(test)]
#[path = "builtin_layout_probe_tests.rs"]
mod builtin_layout_probe_tests;
