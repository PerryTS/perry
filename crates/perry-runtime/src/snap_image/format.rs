//! The image file: what an image run writes and what the emitter reads.
//! Standard library only, so the compiler can include this file unchanged.
//! The reader (`format_read.rs`) is a separate file: the runtime links none of it.
//!
//! A cell is the bytes the heap holds (header included) with every traced
//! slot word zeroed; the slot list names each zeroed word and what it held.
//! A word is a changed root word of the binary: a module global in `.data`/
//! `.bss` or in the program's TLS block.

pub const MAGIC: &[u8; 8] = b"PSIMG001";

/// Forms an image needs that the file does not hold yet (bits of
/// `Image::pending`: shape slab, atom table, thread tokens). An image with
/// a pending bit is incomplete and is never restored.
pub const PENDING: u32 = 0b111;

/// Why an image cannot be used.
pub const STOP_NO_FORM: u8 = 0;
pub const STOP_SLOT_OUTSIDE: u8 = 1;
pub const STOP_NOT_CELL_START: u8 = 2;
/// Names of the class tables, in the order `snap_image_table_sizes` reports them.
pub const CLASS_TABLE_NAMES: [&str; 20] = [
    "declarations",
    "parent_dense",
    "static_methods",
    "static_accessors",
    "string_member_orders",
    "method_bind_lengths",
    "static_method_bind_lengths",
    "registered_class_ids",
    "parents",
    "fetch_parent_kind",
    "generic_origin",
    "extends_error",
    "has_instance",
    "to_string_tag",
    "constructors",
    "constructor_flags",
    "extends_data_view",
    "extends_typed_array",
    "names",
    "lengths",
];
pub const CLASS_TABLE_COUNT: usize = CLASS_TABLE_NAMES.len();
/// A class table grew between the baseline and the cut (`cell` = table index in
/// `CLASS_TABLE_NAMES`, `value` = rows added); one stop per growing table.
pub const STOP_CLASS_TABLES: u8 = 3;
/// A slot-claim key longer than 4 KiB or not UTF-8 (`cell` = claim index).
pub const STOP_CLAIM_KEY: u8 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub cell: u32,
    pub obj_type: u8,
    pub kind: u8,
    /// Slot offset (kind 1, 2) or 0.
    pub off: u32,
    /// The address a slot named (kind 2) or 0.
    pub value: u64,
}

/// Where a word that points outside the cells points.
pub const REGION_STRINGS: u8 = 0;
pub const REGION_LAYOUTS: u8 = 1;
pub const REGION_ATOMS: u8 = 2;
/// Offset from the start of the executable image (any other static).
pub const REGION_EXE: u8 = 3;
/// Offset from the start of the main thread's static TLS block.
pub const REGION_TLS: u8 = 4;

pub const WORD_DATA: u8 = 0;
pub const WORD_TLS: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    /// Not an address: stored as is.
    Scalar(u64),
    /// A reference to cell `index`; `hi` is the NaN-box tag (top 16 bits) or
    /// 0 with `bare` for an untagged address.
    Cell { hi: u16, bare: bool, index: u32 },
    /// A reference into the binary: `region` + `off`.
    Ext {
        hi: u16,
        bare: bool,
        region: u8,
        off: u64,
    },
    /// An address that is neither a cell nor in the binary (malloc, stack).
    /// Never valid inside a cell; a word of this kind needs a form of its own.
    Opaque(u64),
    /// An address inside the persistent-symbol space (a symbol cell): no
    /// form of its own yet, so a word of this kind stops the image.
    Sym(u64),
    /// A word the cell does not hold: restore derives it (kind `DERIVED_*`); zero in the bytes.
    Derived(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub obj_type: u8,
    pub flags: u8,
    pub reserved: u16,
    pub size: u32,
    /// `size` bytes, header included; traced slot words are zero.
    pub bytes: Vec<u8>,
    /// (byte offset from the header, value).
    pub slots: Vec<(u32, Val)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub region: u8,
    pub off: u64,
    pub val: Val,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    pub cells: Vec<Cell>,
    pub words: Vec<Word>,
    /// Reasons the image cannot be used (a cell with no image form, a slot
    /// naming an address that is not a cell).
    pub stops: Vec<Stop>,
    pub pending: u32,
    /// Hot-slot claims at the baseline, and the names claimed from then on
    /// until the cut, in claim order (index = `claims_from` + position).
    pub claims_from: u32,
    pub claims: Vec<Vec<u8>>,
    /// The module-path registry entries the module inits added, for replay.
    pub registry: Vec<RegRec>,
    /// The shape records minted between the baseline and the cut, in mint order.
    pub mint: Vec<MintRec>,
    /// The canonical trie state that appeared between the baseline and the cut.
    pub canon: Option<CanonRec>,
}

fn put_val(o: &mut Vec<u8>, v: &Val) {
    match v {
        Val::Scalar(x) => {
            o.push(0);
            o.extend(x.to_le_bytes());
        }
        Val::Cell { hi, bare, index } => {
            o.push(1);
            o.extend(hi.to_le_bytes());
            o.push(*bare as u8);
            o.extend(u64::from(*index).to_le_bytes());
        }
        Val::Ext {
            hi,
            bare,
            region,
            off,
        } => {
            o.push(2);
            o.extend(hi.to_le_bytes());
            o.push(*bare as u8);
            o.push(*region);
            o.extend(off.to_le_bytes());
        }
        Val::Opaque(x) => {
            o.push(3);
            o.extend(x.to_le_bytes());
        }
        Val::Sym(x) => {
            o.push(4);
            o.extend(x.to_le_bytes());
        }
        Val::Derived(k) => {
            o.push(5);
            o.push(*k);
        }
    }
}

pub fn encode(img: &Image) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend(MAGIC);
    for n in [img.cells.len(), img.words.len(), img.stops.len()] {
        o.extend((n as u32).to_le_bytes());
    }
    for c in &img.cells {
        o.push(c.obj_type);
        o.push(c.flags);
        o.extend(c.reserved.to_le_bytes());
        o.extend(c.size.to_le_bytes());
        o.extend(&c.bytes);
        o.extend((c.slots.len() as u32).to_le_bytes());
        for (off, v) in &c.slots {
            o.extend(off.to_le_bytes());
            put_val(&mut o, v);
        }
    }
    for w in &img.words {
        o.push(w.region);
        o.extend(w.off.to_le_bytes());
        put_val(&mut o, &w.val);
    }
    for t in &img.stops {
        o.extend(t.cell.to_le_bytes());
        o.push(t.obj_type);
        o.push(t.kind);
        o.extend(t.off.to_le_bytes());
        o.extend(t.value.to_le_bytes());
    }
    o.extend(img.pending.to_le_bytes());
    o.extend(img.claims_from.to_le_bytes());
    o.extend(mint_blob(&img.mint));
    match &img.canon {
        None => o.push(0),
        Some(c) => {
            o.push(1);
            o.extend(canon_blob(c));
        }
    }
    o.extend(registry_blob(&img.registry));
    o.extend(claims_blob(&img.claims));

    o
}

/// The claim record as section bytes: count, then (length, bytes) per name.
pub fn claims_blob(claims: &[Vec<u8>]) -> Vec<u8> {
    let mut o = (claims.len() as u32).to_le_bytes().to_vec();
    for k in claims {
        o.extend((k.len() as u32).to_le_bytes());
        o.extend(k);
    }
    o
}

/// The `entries` word of a Map header: the store's inline entry payload.
pub const DERIVED_MAP_ENTRIES: u8 = 1;
/// The `entries` word of a Map store: its own inline entry payload.
pub const DERIVED_STORE_ENTRIES: u8 = 2;
/// A word of a Map store's native key indexes: rebuilt from the entries.
pub const DERIVED_STORE_INDEX: u8 = 3;

/// Bits of `Image::pending` (see `PENDING`).
pub const PENDING_SHAPES: u32 = 0b001;
pub const PENDING_ATOMS: u32 = 0b010;
pub const PENDING_TOKENS: u32 = 0b100;

/// The shape records changed between the baseline and the cut: `value` = the records
/// minted per id band (static, ordinary, dictionary, exotic; 16 bits each), `off` = the
/// records present at the baseline whose state changed, `cell` = those no longer present.
pub const STOP_SHAPES_MINTED: u8 = 5;
/// More than the primary's class-template token was claimed (`value` = next token).
pub const STOP_TOKENS: u8 = 6;
/// An untraced word of a cell holds an address that is not the binary's (`off`, `value`).
pub const STOP_RAW_HEAP_WORD: u8 = 7;
/// A Map keyed by something other than strings and numbers (`obj_type` = the cell).
pub const STOP_MAP_KEYS: u8 = 8;
/// A module-path registry entry that is not Registered or Initialized, or has no form.
pub const STOP_REGISTRY: u8 = 9;

/// One module-path registry entry the module inits added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegRec {
    pub key: Vec<u8>,
    /// Offset of the init function from the start of the executable image.
    pub init_off: u64,
    /// 0 registered, 2 initialized.
    pub status: u8,
    pub exports: Option<Val>,
    pub factory: Option<Val>,
}

fn put_opt(o: &mut Vec<u8>, v: &Option<Val>) {
    match v {
        None => o.push(0),
        Some(v) => {
            o.push(1);
            put_val(o, v);
        }
    }
}

/// The replay record as section bytes: count, then per entry key, init offset, status, exports, factory.
pub fn registry_blob(recs: &[RegRec]) -> Vec<u8> {
    let mut o = (recs.len() as u32).to_le_bytes().to_vec();
    for r in recs {
        o.extend((r.key.len() as u32).to_le_bytes());
        o.extend(&r.key);
        o.extend(r.init_off.to_le_bytes());
        o.push(r.status);
        put_opt(&mut o, &r.exports);
        put_opt(&mut o, &r.factory);
    }
    o
}
/// The shape id or generation counters moved (`value` = ids drawn | generations << 32).
pub const STOP_SHAPE_COUNTERS: u8 = 10;

/// The shape records minted between the baseline and the cut are written, and a table
/// rebuilt from them equals the live one (`value` = records | bytes << 32; `cell` =
/// static | dictionary << 16 records, `off` = ordinary | exotic << 16 records).
pub const STOP_MINT_RECORD: u8 = 11;
/// A record of the mint record whose rebuilt form differs from the live one (`cell` = its
/// index in mint order, `off` = what differs, `value` = its shape id).
pub const STOP_MINT_MISMATCH: u8 = 12;
/// Why a record fails: a dictionary-band record has no form.
pub const MINT_BAD_DICTIONARY: u32 = 1;
/// The facts do not reach the recorded word through the runtime's own lookup.
pub const MINT_BAD_IDENTITY: u32 = 2;
/// A recorded field differs from the live record.
pub const MINT_BAD_FIELDS: u32 = 3;
/// A row or an id is out of mint order, or the word does not carry the recorded row.
pub const MINT_BAD_ORDER: u32 = 4;
/// A lazy image row whose descriptor does not name the recorded word.
pub const MINT_BAD_LAZY_ROW: u32 = 5;

/// The record count and its mint facts, one stop per item (`obj_type` = the item,
/// `value` = its count): the census a capture prints beside the verified record.
pub const STOP_MINT_CENSUS: u8 = 13;
/// A record whose key list is neither an image cell nor binary data (`cell` = the first
/// such record, `value` = its shape id, `off` = how many).
pub const STOP_MINT_KEYS: u8 = 14;
/// A record that keys a canonical list (a trie node).
pub const STOP_MINT_CANONICAL: u8 = 15;
/// A record whose prototype word is neither an image cell, binary data nor a plain scalar.
pub const STOP_MINT_PROTO_WORD: u8 = 16;
/// A runtime layout's first birth whose descriptor cell was not found exactly once.
pub const STOP_MINT_NO_DESC: u8 = 17;
/// A dynamic-row record that leaves a row gap behind the one before it.
pub const STOP_MINT_ROW_GAP: u8 = 18;
/// A record minted by a slot-list successor (holes, or a key-list family that is not the
/// set of records carrying the keys).
pub const STOP_MINT_SUCCESSOR: u8 = 19;
/// The canonical trie freed nodes between the baseline and the cut (a full trace pruned
/// lists): the record cannot carry the free list (`cell` = the first old node a free
/// touched, `off` = how many, `value` = nodes reaped).
pub const STOP_CANON_FREE: u8 = 20;
/// A trie node the record names whose backing is not an image cell (`cell` = the first
/// node, `off` = how many, `value` = its address).
pub const STOP_CANON_ADDR: u8 = 21;
/// Trie nodes whose bucket links say they were not allocated in id order (`cell` = the
/// first node, `off` = how many).
pub const STOP_CANON_ORDER: u8 = 22;
/// The canonical-keys record is written (`value` = new nodes | changed nodes << 32,
/// `off` = its bytes, `cell` = 0 when it reads back equal, 1 when it does not).
pub const STOP_CANON_RECORD: u8 = 23;
/// The canonical-keys record, read back and replayed into a trie rebuilt from the baseline,
/// gives the trie at the cut (`cell` = 0) or not (`cell` = 1, `off` = the first node that
/// differs, `value` = nodes at the cut).
pub const STOP_CANON_REPLAY: u8 = 24;

/// How a record was minted: a dynamic row, a runtime layout's first birth, a lazy image
/// row, or a compile-time shape id.
pub const MINT_ROUTE_DYNAMIC: u8 = 0;
pub const MINT_ROUTE_RUNTIME: u8 = 1;
pub const MINT_ROUTE_LAZY: u8 = 2;
pub const MINT_ROUTE_STATIC: u8 = 3;

/// Flags of `snap_image_mint_replay_flags`.
pub const MINT_FLAG_SUCCESSOR: u8 = 1;
pub const MINT_FLAG_CANONICAL: u8 = 2;
pub const MINT_FLAG_ROW_GAP: u8 = 4;

/// Items of the census stop (`obj_type`).
pub const MINT_CENSUS_ITEMS: usize = 18;

/// One shape record minted between the baseline and the cut, as the facts a replay hands
/// the runtime's mint: the identity facts, the record's attached state and the layout
/// word the mint must return. The id and the row to position the allocators at before
/// the mint are `id` and the row of `word`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MintRec {
    pub id: u32,
    pub word: u32,
    pub keys: u64,
    pub logical: u32,
    pub live: u32,
    pub holes: u32,
    pub generation: u64,
    pub kind: u8,
    pub summary: u8,
    pub integrity: u16,
    pub proto_id: u64,
    pub proto_cell: u64,
    pub rep: u64,
    pub brands: Vec<u64>,
    /// (slot, info) of each ConstFn lane.
    pub constfn: Vec<(u32, u64)>,
    /// Rollback parent, created birth shape, field define target, three integrity targets.
    pub handles: [u32; 6],
    /// The image descriptor a lazy-row record names (index + 1), or 0.
    pub family_desc: u32,
    /// The rep deprecation edges: to-nopointer, to-any, then the tail words.
    pub edges: Vec<u32>,
    /// How it was minted (`MINT_ROUTE_*`).
    pub route: u8,
    /// The canonical key-list node it carries (`u32::MAX` for an owned list).
    pub node: u32,
    /// The key list as a reference valid in the relinked binary (an image cell or binary data),
    /// set where the image is captured; `keys` and `proto_cell` are addresses of this run.
    pub keys_ref: Option<Val>,
    /// The content of the prototype slot (`proto_cell`) as a reference, when the record has a slot.
    pub proto_ref: Option<Val>,
    /// A runtime layout's descriptor: its offset from the start of the data window.
    pub desc_off: Option<u64>,
    /// A runtime layout's (or lazy row's) first birth that drew its family's prototype serial:
    /// the serial it drew (0 = the birth draws none).
    pub fam_serial: u64,
}

/// The record without the references the capture adds after the live record is read.
pub fn mint_without_refs(r: &MintRec) -> MintRec {
    let mut c = r.clone();
    c.keys_ref = None;
    c.proto_ref = None;
    c.desc_off = None;
    c.fam_serial = 0;
    c
}

fn mint_tail<'a>(b: &'a [u8], o: &mut usize, n: usize) -> Option<&'a [u8]> {
    let s = b.get(*o..o.checked_add(n)?)?;
    *o += n;
    Some(s)
}

fn mint_tail_u64(b: &[u8], o: &mut usize) -> Option<u64> {
    Some(u64::from_le_bytes(mint_tail(b, o, 8)?.try_into().ok()?))
}

fn mint_tail_u16(b: &[u8], o: &mut usize) -> Option<u16> {
    Some(u16::from_le_bytes(mint_tail(b, o, 2)?.try_into().ok()?))
}

fn mint_tail_opt(b: &[u8], o: &mut usize) -> Option<Option<Val>> {
    if mint_tail(b, o, 1)?[0] == 0 {
        return Some(None);
    }
    Some(Some(tail_val(b, o)?))
}

/// A value as `put_val` wrote it: the kind byte, then its fields (no derived words).
fn tail_val(b: &[u8], o: &mut usize) -> Option<Val> {
    match mint_tail(b, o, 1)?[0] {
        0 => Some(Val::Scalar(mint_tail_u64(b, o)?)),
        1 => {
            let hi = mint_tail_u16(b, o)?;
            let bare = mint_tail(b, o, 1)?[0] != 0;
            let index = mint_tail_u64(b, o)? as u32;
            Some(Val::Cell { hi, bare, index })
        }
        2 => {
            let hi = mint_tail_u16(b, o)?;
            let bare = mint_tail(b, o, 1)?[0] != 0;
            let region = mint_tail(b, o, 1)?[0];
            let off = mint_tail_u64(b, o)?;
            Some(Val::Ext {
                hi,
                bare,
                region,
                off,
            })
        }
        3 => Some(Val::Opaque(mint_tail_u64(b, o)?)),
        4 => Some(Val::Sym(mint_tail_u64(b, o)?)),
        _ => None,
    }
}

/// The census line of the stops of kind `STOP_MINT_CENSUS`, or `None` without one.
pub fn mint_census_text(stops: &[Stop]) -> Option<String> {
    let mut v: Vec<u64> = Vec::new();
    for _ in 0..MINT_CENSUS_ITEMS {
        v.push(0);
    }
    let mut any = false;
    for s in stops {
        if s.kind == STOP_MINT_CENSUS {
            any = true;
            if let Some(x) = v.get_mut(s.obj_type as usize) {
                *x = s.value;
            }
        }
    }
    if !any {
        return None;
    }
    let g = |i: usize| v.get(i).copied().unwrap_or(0);
    Some(format!(
        "mint census: routes dynamic {}, runtime {}, lazy {}, static {}; canonical key lists {}; successor mints {}; row gaps {}; prototype ids default {}, class {}, mixed {}, unique {}, other {}; unique-prototype counter moved {}, prototype-serial counter moved {}; key lists as image cells {}, as binary data {}; runtime-layout descriptor cells found {}; runtime-layout births that draw the prototype serial {}",
        g(0), g(1), g(2), g(3), g(4), g(5), g(6), g(7), g(8), g(9), g(10), g(11), g(12), g(13), g(14), g(15), g(16), g(17)
    ))
}

/// The mint record as section bytes: count, then per record its fixed fields and its lists.
pub fn mint_blob(recs: &[MintRec]) -> Vec<u8> {
    let mut o = (recs.len() as u32).to_le_bytes().to_vec();
    for r in recs {
        o.extend(r.id.to_le_bytes());
        o.extend(r.word.to_le_bytes());
        o.extend(r.keys.to_le_bytes());
        for n in [r.logical, r.live, r.holes] {
            o.extend(n.to_le_bytes());
        }
        o.extend(r.generation.to_le_bytes());
        o.push(r.kind);
        o.push(r.summary);
        o.extend(r.integrity.to_le_bytes());
        for n in [r.proto_id, r.proto_cell, r.rep] {
            o.extend(n.to_le_bytes());
        }
        o.extend((r.brands.len() as u32).to_le_bytes());
        for b in &r.brands {
            o.extend(b.to_le_bytes());
        }
        o.extend((r.constfn.len() as u32).to_le_bytes());
        for (slot, info) in &r.constfn {
            o.extend(slot.to_le_bytes());
            o.extend(info.to_le_bytes());
        }
        for h in r.handles {
            o.extend(h.to_le_bytes());
        }
        o.extend(r.family_desc.to_le_bytes());
        o.extend((r.edges.len() as u32).to_le_bytes());
        for e in &r.edges {
            o.extend(e.to_le_bytes());
        }
    }
    for r in recs {
        o.push(r.route);
        o.extend(r.node.to_le_bytes());
        put_opt(&mut o, &r.keys_ref);
        put_opt(&mut o, &r.proto_ref);
        match r.desc_off {
            None => o.push(0),
            Some(x) => {
                o.push(1);
                o.extend(x.to_le_bytes());
            }
        }
        o.extend(r.fam_serial.to_le_bytes());
    }
    o
}

/// The records of a mint blob and the bytes it took (bytes after it are ignored).
pub fn parse_mint_blob(b: &[u8]) -> Option<(Vec<MintRec>, usize)> {
    let mut o = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(o..o.checked_add(n)?)?;
        o += n;
        Some(s)
    };
    let le32 = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap_or([0; 4]));
    let le64 = |s: &[u8]| u64::from_le_bytes(s.try_into().unwrap_or([0; 8]));
    let n = le32(take(4)?) as usize;
    let mut v = Vec::new();
    for _ in 0..n {
        let mut r = MintRec {
            id: le32(take(4)?),
            word: le32(take(4)?),
            keys: le64(take(8)?),
            ..MintRec::default()
        };
        r.logical = le32(take(4)?);
        r.live = le32(take(4)?);
        r.holes = le32(take(4)?);
        r.generation = le64(take(8)?);
        r.kind = take(1)?[0];
        r.summary = take(1)?[0];
        r.integrity = u16::from_le_bytes(take(2)?.try_into().ok()?);
        r.proto_id = le64(take(8)?);
        r.proto_cell = le64(take(8)?);
        r.rep = le64(take(8)?);
        for _ in 0..le32(take(4)?) {
            r.brands.push(le64(take(8)?));
        }
        for _ in 0..le32(take(4)?) {
            let slot = le32(take(4)?);
            r.constfn.push((slot, le64(take(8)?)));
        }
        for h in &mut r.handles {
            *h = le32(take(4)?);
        }
        r.family_desc = le32(take(4)?);
        for _ in 0..le32(take(4)?) {
            r.edges.push(le32(take(4)?));
        }
        v.push(r);
    }
    for r in &mut v {
        r.route = mint_tail(b, &mut o, 1)?[0];
        r.node = u32::from_le_bytes(mint_tail(b, &mut o, 4)?.try_into().ok()?);
        r.keys_ref = mint_tail_opt(b, &mut o)?;
        r.proto_ref = mint_tail_opt(b, &mut o)?;
        r.desc_off = match mint_tail(b, &mut o, 1)?[0] {
            0 => None,
            _ => Some(mint_tail_u64(b, &mut o)?),
        };
        r.fam_serial = mint_tail_u64(b, &mut o)?;
    }
    Some((v, o))
}

/// One trie node of the canonical-keys record. A changed node (one present at the
/// baseline) carries its `parent`, `edge_hash` and `len` too: they are the node's
/// identity, which the replay checks against the restored trie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonNode {
    pub id: u32,
    pub parent: u32,
    pub edge_hash: u64,
    pub len: u32,
    pub all_ptr: bool,
    pub published: bool,
    /// The backing (an image cell or binary data), or `Scalar(0)` for none.
    pub addr: Val,
    pub backing_slots: u32,
    pub run: u32,
    pub backing_owner: u32,
}

/// The canonical trie state that appeared between the baseline and the cut: the nodes
/// minted since (in id order), the nodes present at the baseline whose fields changed,
/// and the table scalars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonRec {
    /// Node count at the baseline and at the cut.
    pub begin_len: u32,
    pub cut_len: u32,
    pub new: Vec<CanonNode>,
    pub changed: Vec<CanonNode>,
    /// The list the last append created: backing, count, node.
    pub last_addr: Val,
    pub last_len: u32,
    pub last_node: u32,
    pub minted: u64,
    pub reaped: u64,
}

fn canon_node_put(o: &mut Vec<u8>, n: &CanonNode) {
    o.extend(n.id.to_le_bytes());
    o.extend(n.parent.to_le_bytes());
    o.extend(n.edge_hash.to_le_bytes());
    o.extend(n.len.to_le_bytes());
    o.push(u8::from(n.all_ptr) | u8::from(n.published) << 1);
    put_val(o, &n.addr);
    for x in [n.backing_slots, n.run, n.backing_owner] {
        o.extend(x.to_le_bytes());
    }
}

/// The canonical-keys record as bytes: the two lengths, the new nodes, the changed
/// nodes, then the scalars.
pub fn canon_blob(c: &CanonRec) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend(c.begin_len.to_le_bytes());
    o.extend(c.cut_len.to_le_bytes());
    for list in [&c.new, &c.changed] {
        o.extend((list.len() as u32).to_le_bytes());
        for n in list {
            canon_node_put(&mut o, n);
        }
    }
    put_val(&mut o, &c.last_addr);
    o.extend(c.last_len.to_le_bytes());
    o.extend(c.last_node.to_le_bytes());
    o.extend(c.minted.to_le_bytes());
    o.extend(c.reaped.to_le_bytes());
    o
}

fn canon_u32(b: &[u8], o: &mut usize) -> Option<u32> {
    Some(u32::from_le_bytes(mint_tail(b, o, 4)?.try_into().ok()?))
}

fn canon_node_get(b: &[u8], o: &mut usize) -> Option<CanonNode> {
    let id = canon_u32(b, o)?;
    let parent = canon_u32(b, o)?;
    let edge_hash = mint_tail_u64(b, o)?;
    let len = canon_u32(b, o)?;
    let fl = mint_tail(b, o, 1)?.first().copied()?;
    let addr = tail_val(b, o)?;
    let backing_slots = canon_u32(b, o)?;
    let run = canon_u32(b, o)?;
    let backing_owner = canon_u32(b, o)?;
    Some(CanonNode {
        id,
        parent,
        edge_hash,
        len,
        all_ptr: fl & 1 != 0,
        published: fl & 2 != 0,
        addr,
        backing_slots,
        run,
        backing_owner,
    })
}

/// The canonical-keys record of a blob and the bytes it took.
pub fn parse_canon_blob(b: &[u8]) -> Option<(CanonRec, usize)> {
    let mut o = 0usize;
    let begin_len = canon_u32(b, &mut o)?;
    let cut_len = canon_u32(b, &mut o)?;
    let mut lists = [Vec::new(), Vec::new()];
    for list in lists.iter_mut() {
        for _ in 0..canon_u32(b, &mut o)? {
            list.push(canon_node_get(b, &mut o)?);
        }
    }
    let [new, changed] = lists;
    let last_addr = tail_val(b, &mut o)?;
    let last_len = canon_u32(b, &mut o)?;
    let last_node = canon_u32(b, &mut o)?;
    let minted = mint_tail_u64(b, &mut o)?;
    let reaped = mint_tail_u64(b, &mut o)?;
    Some((
        CanonRec {
            begin_len,
            cut_len,
            new,
            changed,
            last_addr,
            last_len,
            last_node,
            minted,
            reaped,
        },
        o,
    ))
}
