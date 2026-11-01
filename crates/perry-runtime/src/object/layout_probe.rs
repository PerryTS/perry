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
#[path = "layout_probe_indexed.rs"]
mod indexed;
#[path = "layout_probe_site_memo.rs"]
pub(crate) mod site_memo;
#[path = "layout_probe_store.rs"]
mod store_target;
#[path = "layout_probe_unloaded.rs"]
mod unloaded;

use super::shapes_store::{ShapeRecord, FLAT_BASE_OFFSET};
use crate::codegen_abi::LayoutWord;
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
/// A bucket entry whose key is a data property stored at an inline slot of
/// every receiver of the record other than its position (a layout that
/// separates key order from storage, #12637: a family's coloured key): the
/// slot is the position's index word, and the hit is one load. Never with
/// [`TEXT_ENTRY`] or [`ALIAS_ENTRY`], whose index words hold a hash or atom.
const SLOT_ENTRY: u32 = 1 << 28;
/// A bucket entry whose key is a public accessor stored at an inline slot of
/// every receiver of the record: the slot is the position's index word, and
/// the accessor pair is one load (`accessor_at` would read exactly that
/// slot). A fact of the immutable list, its attributes and the inline slot
/// count, like [`SLOT_ENTRY`]; never with [`TEXT_ENTRY`] or [`ALIAS_ENTRY`].
/// The hit path reads such a key as it reads any accessor: it declines, and
/// the site slow entry (`js_object_get_field_slow`) calls the getter.
const ACCESSOR_ENTRY: u32 = 1 << 27;
/// The position bits of a bucket entry.
const POS_BITS: u32 = ACCESSOR_ENTRY - 1;
/// [`hit_value`]'s slot when the hit is no single load.
const NO_LOAD: u32 = u32::MAX;
/// The bits of a probe word that are not the position table's address.
const PROBE_FLAGS: u64 = 7 | PROBE_CHECKED | PROBE_ATTRS;

/// A derived key index: `count` alias words, then `mask + 1` buckets of key
/// positions, `u32::MAX` = empty, open addressing from `hash & mask`. The
/// alias word of a [`TEXT_ENTRY`] position holds its text's hash, of a
/// [`SLOT_ENTRY`] position its inline slot.
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
/// mixed creation id; `None` for a word no site key can equal.
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

/// The colour slot (plus one) of a coloured key whose atom is an immutable
/// runtime image that cannot carry the colour in its flags (a spelling the
/// runtime also names: its strong image wins the link over the program's
/// coloured weak one), by mask bit. Zero for every other bit.
#[thread_local]
static COLOUR_SLOTS: [std::cell::Cell<u32>; 127] = [const { std::cell::Cell::new(0) }; 127];

/// Open-addressed index (mask bit plus one, 0 empty) of the flagless
/// coloured atoms by their creation hash: [`COLOUR_ATOMS`] and
/// [`COLOUR_SLOTS`] inverted, so the mint finds a key's colour from its atom.
#[thread_local]
static COLOUR_BY_HASH: [std::cell::Cell<u8>; 256] = [const { std::cell::Cell::new(0) }; 256];

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
        } else if let Some(a) = crate::string::atom_lookup(bytes, hash) {
            // SAFETY: `a` is a live atom of this agent.
            unsafe { seed_flagless_colour(a, hash, bit, slot) };
        }
    }
}

/// Give the coloured key `bit` the immutable atom `a` that cannot carry its
/// colour in its flags: the atom, its slot and its place in the index.
unsafe fn seed_flagless_colour(a: *const crate::StringHeader, hash: u64, bit: u32, slot: u32) {
    if bit >= 127
        || slot >= 256
        || !crate::string::is_program_atom(a)
        || crate::string::atom_colour(a).is_some()
    {
        return;
    }
    if COLOUR_ATOMS[bit as usize].get() == a as usize && COLOUR_SLOTS[bit as usize].get() != 0 {
        // Seeded already: the index is built once per agent and never grows.
        return;
    }
    COLOUR_ATOMS[bit as usize].set(a as usize);
    COLOUR_SLOTS[bit as usize].set(slot + 1);
    let mut at = (hash & 255) as usize;
    for _ in 0..256 {
        if COLOUR_BY_HASH[at].get() == 0 {
            COLOUR_BY_HASH[at].set(bit as u8 + 1);
            return;
        }
        at = (at + 1) & 255;
    }
}

/// The (bit, slot) colour of a program atom `a` that carries none in its
/// flags: the one [`seed_flagless_colour`] gave it, if any.
unsafe fn flagless_colour(a: *const crate::StringHeader) -> Option<(u32, u32)> {
    if !crate::string::is_program_atom(a) {
        return None;
    }
    let mut at = (crate::string::property_atom_hash(a) & 255) as usize;
    for _ in 0..256 {
        let b = COLOUR_BY_HASH[at].get();
        if b == 0 {
            return None;
        }
        let bit = b as u32 - 1;
        if COLOUR_ATOMS[bit as usize].get() == a as usize {
            return COLOUR_SLOTS[bit as usize]
                .get()
                .checked_sub(1)
                .map(|slot| (bit, slot));
        }
        at = (at + 1) & 255;
    }
    None
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
        let Some((bit, slot)) = crate::string::atom_colour(a).or_else(|| flagless_colour(a)) else {
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
                    != crate::object::field_rep::REP_SPECIAL)
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

#[path = "layout_probe/derive_word.rs"]
mod derive_word;
use derive_word::derive_word;

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

/// The inline slot of a data key (no accessor, not private) at position
/// `pos` of `rec`'s list `keys` that is stored at another slot than its
/// position ([`SLOT_ENTRY`]), or `None`.
unsafe fn inline_slot(
    rec: &ShapeRecord,
    keys: *const crate::array::ArrayHeader,
    pos: u32,
) -> Option<u32> {
    use crate::object::key_attrs::{
        key_storage_slot, keys_entry, ENTRY_ACCESSOR_MASK, ENTRY_PRIVATE, SUMMARY_ACCESSOR,
        SUMMARY_PRIVATE,
    };
    if rec.summary() & (SUMMARY_ACCESSOR | SUMMARY_PRIVATE) != 0
        && keys_entry(keys, pos) & (ENTRY_ACCESSOR_MASK | ENTRY_PRIVATE) != 0
    {
        return None;
    }
    let slot = key_storage_slot(keys, pos);
    (slot != pos && slot < rec.live_inline_slot_count && slot != NO_LOAD).then_some(slot)
}

/// May a TEXT entry of `rec`'s list prove absence (see `derive_word`)?
unsafe fn text_proves_absence(rec: &ShapeRecord) -> bool {
    if rec.object_kind() == super::ShapeObjectKind::Ordinary {
        return true;
    }
    let dir = super::ordinary_dir_addr();
    match object_prototype(dir) {
        Some(p) => std::ptr::eq(record_of(dir, LayoutWord((*p).parent_class_id)), rec),
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
unsafe fn record_of(dir: *const u8, word: LayoutWord) -> *mut ShapeRecord {
    let base = *(dir.add(FLAT_BASE_OFFSET) as *const *mut ShapeRecord);
    let ids = *(dir.add(FLAT_BASE_OFFSET + 8) as *const usize);
    let rel = (word.0 & crate::codegen_abi::LAYOUT_PROBE_INDEX_MASK)
        .wrapping_sub(crate::codegen_abi::LAYOUT_TAG) as usize;
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
    own_entry(r, key_bits, hash).map(|(pos, _)| pos)
}

/// [`own_position`] and the inline slot of the accessor pair there when the
/// index names one ([`ACCESSOR_ENTRY`]), else [`NO_LOAD`].
#[inline(always)]
unsafe fn own_entry(r: *mut ShapeRecord, key_bits: u64, hash: u64) -> Result<(u32, u32), bool> {
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
                return Ok((i, NO_LOAD));
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
            let pair = if pos & ACCESSOR_ENTRY != 0 {
                KeyIndex::word(table, p) as u32
            } else {
                NO_LOAD
            };
            return Ok((p, pair));
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

/// The data value at key position `pos` of `obj` (record `r`): one load of
/// inline slot `load` when the record's facts name it (the position for a
/// [`DIRECT_ENTRY`] or below `POSBOUND` in an unindexed list, the index word
/// of a [`SLOT_ENTRY`]), else ([`NO_LOAD`]) [`value_at`]. A hole is the
/// slot's value either way, which the site treats as a decline.
#[inline(always)]
unsafe fn hit_value(obj: *const ObjectHeader, r: *const ShapeRecord, pos: u32, load: u32) -> f64 {
    if load != NO_LOAD {
        return *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + load as usize * 8)
            as *const f64);
    }
    value_at(obj, r, pos)
}

/// The slot [`hit_value`] loads for bucket entry `entry` (position `i`) of
/// `table`.
#[inline(always)]
unsafe fn entry_load(table: *const KeyIndex, entry: u32, i: u32) -> u32 {
    if entry & DIRECT_ENTRY != 0 {
        i
    } else if entry & SLOT_ENTRY != 0 {
        KeyIndex::word(table, i) as u32
    } else {
        NO_LOAD
    }
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
    if record_of(dir, LayoutWord((*obj).parent_class_id)).is_null() {
        return hole();
    }
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
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
    probe_get_at::<ABSENT, false, false>(dir, obj_biased, key_bits, hash, std::ptr::null())
}

/// Memo fronts use the fourth register for the memo. Their key is a
/// property atom; only hash-using paths load its existing creation hash.
#[inline(always)]
unsafe fn site_key_hash<const LAZY_HASH: bool>(key_bits: u64, hash: u64) -> u64 {
    if LAZY_HASH {
        // Named memo fronts accept canonical property atoms, never an SSO
        // immediate. Keep that unsafe entry contract explicit before using
        // the heap-string accessor; materialization would violate this leaf.
        let key = crate::JSValue::from_bits(key_bits);
        debug_assert!(!key.is_short_string(), "memo keys must be property atoms");
        crate::string::property_atom_hash(key.as_string_ptr())
    } else {
        hash
    }
}

/// Actual table ownership was proved by the front on this NoCollect path.
/// Keep the operation's inherited-call/verify register needs out of receivers
/// whose ordinary record already answers without an owned table.
#[inline(never)]
unsafe fn owned_get<const ABSENT: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
    table: *const crate::object::overflow::Table,
) -> f64 {
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || (*meta).elements != 0
        || (*meta).flags
            & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
            != 0
    {
        return hole();
    }
    let hash = site_key_hash::<LAZY_HASH>(key_bits, hash);
    let value = match crate::object::overflow::find_hashed(table, key_bits, hash) {
        Some(entry) => {
            if *crate::object::overflow::attrs_at(table, entry) as u8
                & crate::object::key_attrs::ENTRY_ACCESSOR_MASK
                != 0
            {
                hole()
            } else {
                // The front proved this current ordinary record and actual
                // table; find_hashed just proved the live entry. Consume those
                // same facts through the shared checked entry reader.
                let record = super::ShapeRecordRef(std::ptr::NonNull::new_unchecked(r));
                crate::object::overflow::get_found_from_record(table, obj, entry, record)
                    .map(f64::from_bits)
                    .unwrap_or_else(hole)
            }
        }
        // The actual table has proved own absence. Continue through
        // the existing current-shape prototype walk, without deriving
        // a redundant receiver-key index or learning an overflow memo.
        None => inherited_get::<ABSENT>(dir, obj, r, key_bits, hash),
    };
    if ABSENT {
        verify::answer(obj as usize, key_bits, value)
    } else {
        value
    }
}

/// The own-data front. With `SITE` (a compile-time choice) every own miss
/// first asks the site's shape/slot memo (`site_memo`), then continues
/// exactly as the stateless probe; an own hit never reads the memo.
#[inline(always)]
unsafe fn probe_get_at<const ABSENT: bool, const SITE: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    hash: u64,
    memo: *const site_memo::ReadSiteMemo,
) -> f64 {
    // The hit path keeps to caller-saved registers: every other outcome is a
    // tail call, so the leaf saves nothing on a hit.
    let obj =
        (obj_biased as usize).wrapping_add(perry_abi::RECEIVER_HANDLE_FLOOR) as *const ObjectHeader;
    let word = (*obj).parent_class_id;
    let r = record_of(dir, LayoutWord(word));
    if r.is_null() {
        return array_named_get::<ABSENT>(
            dir,
            obj,
            key_bits,
            site_key_hash::<LAZY_HASH>(key_bits, hash),
        );
    }
    // The owned table is the complete own-key/attribute authority. Asking
    // its existing index first avoids deriving a second record-key index.
    if word & crate::object::overflow::SHADOW_BIT != 0
        && (*r).present()
        && (*r).object_kind() == super::ShapeObjectKind::Ordinary
    {
        // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
        if !meta.is_null() {
            let table = (*meta).overflow as usize as *const crate::object::overflow::Table;
            if !table.is_null() {
                return owned_get::<ABSENT, LAZY_HASH>(dir, obj, r, key_bits, hash, table);
            }
        }
    }
    let probe = (*r).probe;
    if probe & (PROBE_READY | PROBE_DECLINE | PROBE_CHECKED) != PROBE_READY {
        if SITE {
            return site_memo::miss_underived::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash, memo);
        }
        return probe_get_underived::<ABSENT>(
            dir,
            obj,
            LayoutWord(word),
            r,
            key_bits,
            site_key_hash::<LAZY_HASH>(key_bits, hash),
        );
    }
    let count = (*r).logical_key_count;
    if count == 0 {
        return if SITE {
            site_memo::miss_absent::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash, memo)
        } else {
            absent_from::<ABSENT>(
                dir,
                obj,
                key_bits,
                site_key_hash::<LAZY_HASH>(key_bits, hash),
            )
        };
    }
    if probe & !PROBE_FLAGS != 0 {
        return indexed::get::<ABSENT, SITE, LAZY_HASH>(dir, obj, r, key_bits, hash, memo);
    }
    // An unindexed list has no accessor or private entry.
    let words = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
    let count = (*r).logical_key_count;
    let bound = (*r).position_bound_raw();
    let mut pos = 0u32;
    loop {
        if *words.add(pos as usize) == key_bits {
            break;
        }
        pos += 1;
        if pos == count {
            return if SITE {
                site_memo::miss_absent::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash, memo)
            } else {
                absent_from::<ABSENT>(
                    dir,
                    obj,
                    key_bits,
                    site_key_hash::<LAZY_HASH>(key_bits, hash),
                )
            };
        }
    }
    let load = if pos < bound { pos } else { NO_LOAD };
    unloaded::finish::<ABSENT, SITE, LAZY_HASH>(dir, obj, key_bits, hash, pos, load, memo)
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

/// Is position `pos` of record `r` an accessor or a private name (not
/// [`plain_data_at`])? Out of line: the chain walk asks it only for a hole.
#[cold]
#[inline(never)]
unsafe fn attributed_at(r: *const ShapeRecord, pos: u32) -> bool {
    !plain_data_at(r, pos)
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
            // A plain function's [[Prototype]] is its shape's identity
            // (`%Function.prototype%` until a link moves it).
            (
                (*c).props as *const ObjectHeader,
                plain
                    .then(|| crate::object::shapes::shape_proto_id((*c).shape_id))
                    .flatten(),
                plain,
            )
        }
        _ => return hole(),
    };
    if !named.is_null() {
        let word = (*named).parent_class_id;
        let r = record_of(dir, LayoutWord(word));
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
            let rn = record_of(dir, LayoutWord(w));
            if rn.is_null() || w & crate::object::overflow::SHADOW_BIT != 0 {
                return hole();
            }
            let pw = probe_word(rn);
            if pw & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS {
                return hole();
            }
            match list_position(rn, pw, key_bits, hash) {
                Ok((pos, load)) => return hit_value(p, rn, pos, load),
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
    chain_from::<ABSENT>(dir, std::ptr::null(), next, key_bits, hash)
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
    word: LayoutWord,
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
    word: LayoutWord,
    r: *mut ShapeRecord,
    probe: u64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    // Every outcome is a tail call or a constant, so the caller's hit path
    // keeps its frameless form.
    if word.0 & crate::object::overflow::SHADOW_BIT != 0 {
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
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
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

#[path = "layout_probe/inherited.rs"]
mod inherited;
use inherited::absent_from;
use inherited::chain_from;
use inherited::inherited_get;
use inherited::link_of;
use inherited::list_position;
use inherited::listed;
use inherited::object_prototype;
use inherited::prototype_of;
use inherited::realm_default_link;
use inherited::walks_from;
use inherited::Next;

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
    if crate::gc_type_is_not_object!((*header).obj_type) {
        return None;
    }
    let mut cur = obj;
    let mut rc = record_of(dir, LayoutWord((*cur).parent_class_id)) as *const ShapeRecord;
    if rc.is_null() || !(*rc).present() {
        return None;
    }
    for hop in 0..=MAX_HOPS {
        if hop > 0
            || (*rc).object_kind() == super::ShapeObjectKind::Ordinary
            || listed(cur, (*rc).object_kind())
        {
            match own_entry(rc as *mut ShapeRecord, key_bits, hash) {
                Ok((pos, pair)) => return accessor_in(cur, rc, pos, pair),
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
        let rn = record_of(dir, LayoutWord((*next).parent_class_id)) as *const ShapeRecord;
        if rn.is_null() || !(*rn).present() {
            return None;
        }
        match (*rn).object_kind() {
            super::ShapeObjectKind::Ordinary => {}
            kind if listed(next, kind) => {}
            _ => {
                // A builtin prototype's list holds what the program defined
                // on it; an accessor found there is that accessor.
                return match own_entry(rn as *mut ShapeRecord, key_bits, hash) {
                    Ok((pos, pair)) => accessor_in(next, rn, pos, pair),
                    Err(_) => None,
                };
            }
        }
        cur = next;
        rc = rn;
    }
    None
}

/// The accessor at key position `pos` of `obj` (record `r`): one load of
/// inline slot `pair` when the record's index names it ([`ACCESSOR_ENTRY`]),
/// else ([`NO_LOAD`]) [`accessor_at`].
#[inline(always)]
unsafe fn accessor_in(
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
    pos: u32,
    pair: u32,
) -> Option<crate::object::accessor_pair::Accessor> {
    if pair == NO_LOAD {
        return accessor_at(obj, r, pos);
    }
    let bits = *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + pair as usize * 8)
        as *const u64);
    Some(crate::object::accessor_pair::pair_of_value(bits).unwrap_or_default())
}

/// The accessor at key position `pos` of `obj` (record `r`), if that key is
/// an accessor.
#[inline(never)]
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
    let r = record_of(dir, LayoutWord((*obj).parent_class_id));
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
        if crate::gc_type_is_object!((*h).obj_type)
            && (*h).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT == 0
        {
            let r = record_of(dir, LayoutWord((*obj).parent_class_id));
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

/// The probe's presence mode (`in`, `Object.hasOwn`,
/// `hasOwnProperty`): `1` when `key` is present on the object at `obj_bits`
/// (own, or along its prototype chain unless `own_only`), `0` when the
/// layouts prove it absent, `-1` when the probe cannot answer (an exotic
/// receiver, an index-like or uninterned key, a builtin prototype's unlisted
/// names, an unknown prototype). A hole slot is an absent key (the key view);
/// an accessor is present. GC leaf.
///
/// # Safety
/// `obj_bits` and `key_bits` any NaN-boxed values.
pub(crate) unsafe fn probe_has(obj_bits: u64, key_bits: u64, own_only: bool) -> i32 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return -1;
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return -1;
    }
    // A key with no property atom (its text was never interned as one) can
    // still match a list word written as that very string: presence only,
    // since a miss cannot prove absence for an uninterned key.
    let (key, hash, exact) = match computed_key(key_bits) {
        Some((k, h)) => (k, h, true),
        None if key_bits & TAG_MASK == STRING_TAG => {
            let s = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            let text =
                std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
            if index_like(text) {
                return -1;
            }
            (
                key_bits,
                crate::object::key_bytes_hash(text.as_ptr(), text.len()),
                false,
            )
        }
        None => return -1,
    };
    let absent = if exact { 0 } else { -1 };
    let dir = super::ordinary_dir_addr();
    let mut cur = payload as *const ObjectHeader;
    for _ in 0..MAX_HOPS {
        let w = (*cur).parent_class_id;
        let r = record_of(dir, LayoutWord(w));
        if r.is_null() {
            return -1;
        }
        match own_position(r, key, hash) {
            Ok(pos) => {
                let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
                let entry = crate::object::key_attrs::keys_entry(keys, pos);
                // A private name shares its spelling with a public key; the
                // complete operation tells them apart.
                if entry & crate::object::key_attrs::ENTRY_PRIVATE != 0 {
                    return -1;
                }
                if entry & crate::object::key_attrs::ENTRY_ACCESSOR_MASK != 0 {
                    return 1;
                }
                let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
                let v = if slot < (*r).live_inline_slot_count {
                    *((cur as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
                        as *const u64)
                } else {
                    crate::object::spill::spill_get_present(cur as usize, slot as usize)
                        .unwrap_or(TAG_HOLE)
                };
                if v != TAG_HOLE {
                    return 1;
                }
                // A hole slot: the key is absent here; a re-add lives in the
                // overflow.
            }
            Err(false) => return -1,
            // A class declaration's prototype holds its symbol-named
            // declared methods as virtual own keys its list does not name:
            // its list proves no symbol absent.
            Err(true)
                if key & TAG_MASK != STRING_TAG
                    && (*cur).class_id != 0
                    && class_prototype_object(cur) =>
            {
                return -1
            }
            Err(true) => {}
        }
        if w & crate::object::overflow::SHADOW_BIT != 0 {
            // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _));
            if !meta.is_null() && (*meta).overflow != 0 {
                let t = (*meta).overflow as usize as *const crate::object::overflow::Table;
                if let Some(i) = crate::object::overflow::find_hashed(t, key, hash) {
                    return if *crate::object::overflow::attrs_at(t, i) as u8
                        & crate::object::key_attrs::ENTRY_PRIVATE
                        != 0
                    {
                        -1
                    } else {
                        1
                    };
                }
            }
        }
        // Absence on `cur` is proven only by a layout that lists every own
        // key (the builtin prototypes are static layouts).
        if !walks_from(cur, r) {
            return -1;
        }
        if own_only {
            // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
            // An Array subclass's elements store holds its own index keys and
            // `length`, which no key list names.
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _));
            if !meta.is_null() && (*meta).elements != 0 {
                return -1;
            }
            return absent;
        }
        cur = match prototype_of(dir, cur, r) {
            Next::Object(p) => p,
            Next::End => return absent,
            Next::Unknown => return -1,
        };
    }
    -1
}

/// Is `obj` (class id nonzero) a class declaration's or a class
/// evaluation's prototype object?
#[cold]
#[inline(never)]
unsafe fn class_prototype_object(obj: *const ObjectHeader) -> bool {
    crate::object::class_value::class_decl_prototype_link((*obj).class_id) as usize == obj as usize
        || crate::object::field_get_set::class_evaluation_prototype_class_id(obj as usize).is_some()
}

/// The store twin of [`perry_probe_get`] (§3.7, the stores' probe path):
/// `obj.key = value` when the receiver's own layout holds `key` as a plain
/// writable data property in an inline slot, or its overflow holds it as a
/// plain writable entry of its own: one barriered store. Returns 1 when it
/// stored. Returns 0 for everything the site's complete `[[Set]]` must do:
/// a key add (the canonical tree or the overflow), a setter or a
/// non-writable property along the chain, a hole slot (an absent key, per
/// the key view), a frozen, exotic or per-object receiver, a slot whose
/// representation the value would widen.
///
/// GC leaf: no allocation on the heap, no collection, no user code.
///
/// # Safety
/// `dir` as for [`perry_probe_get`]; `obj_bits` any NaN-boxed value;
/// `key_bits` a property atom or a symbol; `hash` its hash.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_set(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    hash: u64,
    value: f64,
) -> i32 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return 0;
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return 0;
    }
    let obj = payload as *mut ObjectHeader;
    let word = (*obj).parent_class_id;
    let r = record_of(dir, LayoutWord(word));
    if r.is_null() || (*r).object_kind() != super::ShapeObjectKind::Ordinary {
        return 0;
    }
    let gc = crate::value::addr_class::honest_header(payload);
    if gc._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return 0;
    }
    let bits = if value.to_bits() == POINTER_TAG {
        crate::value::TAG_UNDEFINED
    } else {
        value.to_bits()
    };
    // A clear shadow bit proves there is no overflow. Otherwise its
    // entries, when present, own every current key and attribute.
    if word & crate::object::overflow::SHADOW_BIT != 0 {
        // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
        if !meta.is_null() && (*meta).overflow != 0 {
            return store_target::overflow(
                obj,
                r,
                (*meta).overflow as usize as *mut _,
                key_bits,
                hash,
                bits,
            );
        }
    }
    match own_position(r, key_bits, hash) {
        Ok(pos) => {
            let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
            if (*r).summary()
                & (crate::object::key_attrs::SUMMARY_BLOCKS_STORE
                    | crate::object::key_attrs::SUMMARY_PRIVATE)
                != 0
                && !crate::object::key_attrs::entry_is_plain_writable_data(
                    crate::object::key_attrs::keys_entry(keys, pos),
                )
            {
                return 0;
            }
            let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
            if slot < (*r).live_inline_slot_count {
                let address = (obj as *const u8)
                    .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
                    .cast::<u64>();
                // Only the static record may admit an absent key's hole fill.
                if *address == TAG_HOLE
                    && !hole_fill_admitted(dir, obj, r, keys, pos, key_bits, hash)
                {
                    return 0;
                }
            }
            store_target::checked_slot(obj, r, slot, bits)
        }
        Err(true) => i32::from(probe_add(dir, obj, r, key_bits, hash, bits)),
        Err(false) => 0,
    }
}

#[path = "layout_probe/write.rs"]
mod write;
use write::hole_fill_admitted;
pub(crate) use write::layouts_admit_add;
pub(crate) use write::{function_link_admits_add, function_put_plan, FunctionPut};
use write::probe_add;

#[path = "layout_probe/computed.rs"]
mod computed;
use computed::computed_key;
#[cfg(test)]
pub use computed::perry_probe_get_keyed;
pub use computed::perry_probe_get_value;

#[path = "layout_probe/diagnostics.rs"]
mod diagnostics;
pub(crate) use diagnostics::diag_accessor;
pub(crate) use diagnostics::diag_enabled;
pub(crate) use diagnostics::diag_explain;

/// This agent's ordinary shape directory (tests).
#[cfg(test)]
pub(crate) fn test_dir() -> *const u8 {
    super::ordinary_dir_addr()
}

/// Key position `pos` of `obj`'s record's list (tests).
#[cfg(test)]
pub(crate) unsafe fn test_list_word(obj: *const ObjectHeader, pos: usize) -> u64 {
    let r = record_of(
        super::ordinary_dir_addr(),
        LayoutWord((*obj).parent_class_id),
    );
    assert!(!r.is_null(), "the object has an ordinary record");
    *(crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64).add(pos)
}

/// Probe verify mode (`PERRY_PROBE_VERIFY=1`), a debugging instrument: every
/// answer the probe gives (a value or a proven absence) is held back, the
/// probe declines, and the site's slow entry checks the held answer against
/// the complete `[[Get]]` (`Reflect.get`) before it runs. The first
/// disagreements print the key, both answers and the receiver's chain.
#[path = "layout_probe/verify.rs"]
pub(crate) mod verify;

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

#[cfg(test)]
#[path = "layout_probe_overflow_tests.rs"]
mod overflow_tests;
