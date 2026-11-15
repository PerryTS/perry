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
/// The class tables grew between the baseline and the cut (`value` = rows).
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
/// Or-ed into the region: written only by the capture's own collect (the
/// word equals the baseline in the copy taken first at the cut).
pub const WORD_COLLECT: u8 = 2;

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
