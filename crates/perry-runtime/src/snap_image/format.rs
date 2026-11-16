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

/// The names of a claim blob (bytes after it are ignored).
pub fn parse_claims_blob(b: &[u8]) -> Option<Vec<Vec<u8>>> {
    let u32_at = |o: usize| -> Option<usize> {
        b.get(o..o + 4)
            .map(|s| u32::from_le_bytes(s.try_into().unwrap_or([0; 4])) as usize)
    };
    let n = u32_at(0)?;
    let (mut o, mut v) = (4usize, Vec::new());
    for _ in 0..n {
        let l = u32_at(o)?;
        v.push(b.get(o + 4..o + 4 + l)?.to_vec());
        o += 4 + l;
    }
    Some(v)
}

/// Whether `claims` (claimed since index `from` in some process) are exactly
/// what this process has claimed since `from`, with the same indices.
pub fn claims_replay_matches(from: u32, claims: &[Vec<u8>], now: &[(Vec<u8>, u32)]) -> bool {
    claims.len() == now.len()
        && claims
            .iter()
            .zip(now)
            .enumerate()
            .all(|(i, (k, (nk, ni)))| k == nk && *ni == from + i as u32)
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
        let mut r = MintRec { id: le32(take(4)?), word: le32(take(4)?), keys: le64(take(8)?), ..MintRec::default() };
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
    Some((v, o))
}
