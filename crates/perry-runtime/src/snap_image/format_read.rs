//! Reader and dump of the image file (tests and tools).
use super::format::*;

struct Rd<'a>(&'a [u8], usize);

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self
            .1
            .checked_add(n)
            .filter(|&e| e <= self.0.len())
            .ok_or("truncated image")?;
        let s = &self.0[self.1..end];
        self.1 = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn val(&mut self) -> Result<Val, String> {
        Ok(match self.u8()? {
            0 => Val::Scalar(self.u64()?),
            1 => {
                let (hi, bare) = (self.u16()?, self.u8()? != 0);
                Val::Cell {
                    hi,
                    bare,
                    index: self.u64()? as u32,
                }
            }
            2 => {
                let (hi, bare, region) = (self.u16()?, self.u8()? != 0, self.u8()?);
                Val::Ext {
                    hi,
                    bare,
                    region,
                    off: self.u64()?,
                }
            }
            3 => Val::Opaque(self.u64()?),
            4 => Val::Sym(self.u64()?),
            k => return Err(format!("bad value kind {k}")),
        })
    }
}

pub fn decode(b: &[u8]) -> Result<Image, String> {
    let mut r = Rd(b, 0);
    if r.take(8)? != MAGIC {
        return Err("not an image file".into());
    }
    let (nc, nw, ns) = (r.u32()?, r.u32()?, r.u32()?);
    let mut img = Image::default();
    for _ in 0..nc {
        let (obj_type, flags, reserved, size) = (r.u8()?, r.u8()?, r.u16()?, r.u32()?);
        let bytes = r.take(size as usize)?.to_vec();
        let n = r.u32()?;
        let mut slots = Vec::new();
        for _ in 0..n {
            let off = r.u32()?;
            slots.push((off, r.val()?));
        }
        img.cells.push(Cell {
            obj_type,
            flags,
            reserved,
            size,
            bytes,
            slots,
        });
    }
    for _ in 0..nw {
        let region = r.u8()?;
        let off = r.u64()?;
        img.words.push(Word {
            region,
            off,
            val: r.val()?,
        });
    }
    for _ in 0..ns {
        let cell = r.u32()?;
        let (obj_type, kind) = (r.u8()?, r.u8()?);
        let off = r.u32()?;
        img.stops.push(Stop {
            cell,
            obj_type,
            kind,
            off,
            value: r.u64()?,
        });
    }
    img.pending = r.u32()?;
    Ok(img)
}

/// The summary the dump tools print: cells per type, bytes, words per class,
/// stops, pending forms. Sorted, so two runs diff.
pub fn dump(img: &Image) -> String {
    use std::collections::BTreeMap;
    let mut by: BTreeMap<u8, (u64, u64)> = BTreeMap::new();
    for c in &img.cells {
        let e = by.entry(c.obj_type).or_default();
        e.0 += 1;
        e.1 += u64::from(c.size);
    }
    let mut s = String::new();
    for (t, (n, b)) in &by {
        s += &format!("cell type={t} n={n} bytes={b}\n");
    }
    let slots: usize = img.cells.iter().map(|c| c.slots.len()).sum();
    let bytes: u64 = by.values().map(|v| v.1).sum();
    s += &format!("cells={} bytes={bytes} slots={slots}\n", img.cells.len());
    let mut w: BTreeMap<(u8, &str), u64> = BTreeMap::new();
    for x in &img.words {
        let k = match x.val {
            Val::Scalar(_) => "scalar",
            Val::Cell { .. } => "cell",
            Val::Ext { .. } => "ext",
            Val::Opaque(_) => "opaque",
            Val::Sym(_) => "sym",
        };
        *w.entry((x.region, k)).or_default() += 1;
    }
    for ((r, k), n) in &w {
        s += &format!(
            "words region={} kind={k} n={n}\n",
            if *r & WORD_TLS == 0 { "data" } else { "tls" }
        );
    }
    s += &format!("words={}\n", img.words.len());
    for t in &img.stops {
        s += &format!(
            "stop cell={} type={} kind={} off={} value={:#x}\n",
            t.cell, t.obj_type, t.kind, t.off, t.value
        );
    }
    for (bit, name) in [(1, "shape slab"), (2, "atom table"), (4, "thread tokens")] {
        if img.pending & bit != 0 {
            s += &format!("pending {name}\n");
        }
    }
    s
}
