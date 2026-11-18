//! Reader and dump of the image file (tests and tools).
use super::format::*;

struct Rd<'a>(&'a [u8], usize);

impl Rd<'_> {
    fn left(&self) -> usize {
        self.0.len() - self.1
    }
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
            5 => Val::Derived(self.u8()?),
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
    img.claims_from = r.u32()?;
    let rest = r.take(r.left())?;
    let (mint, mint_used) = parse_mint_blob(rest).ok_or("bad mint record")?;
    img.mint = mint;
    let rest = rest.get(mint_used..).ok_or("bad canonical-keys record")?;
    let (canon, canon_used) = match rest.first() {
        Some(0) => (None, 1),
        Some(1) => {
            let (c, u) = parse_canon_blob(rest.get(1..).ok_or("bad canonical-keys record")?)
                .ok_or("bad canonical-keys record")?;
            (Some(c), 1 + u)
        }
        _ => return Err("bad canonical-keys record".into()),
    };
    img.canon = canon;
    let rest = rest.get(canon_used..).ok_or("bad registry record")?;
    let (registry, used) = parse_registry_blob(rest).ok_or("bad registry record")?;
    img.registry = registry;
    img.claims =
        parse_claims_blob(rest.get(used..).ok_or("bad claim record")?).ok_or("bad claim record")?;
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
            Val::Derived(_) => "derived",
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
    s += &format!("mint-records={}\n", img.mint.len());
    s += &format!("registry={}\n", img.registry.len());
    for r in &img.registry {
        s += &format!(
            "registry-entry key={} status={}\n",
            String::from_utf8_lossy(&r.key).escape_debug(),
            r.status
        );
    }
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

/// The entries of a registry blob and the bytes it took (bytes after it are ignored).
pub fn parse_registry_blob(b: &[u8]) -> Option<(Vec<RegRec>, usize)> {
    let mut r = Rd(b, 0);
    let n = r.u32().ok()?;
    let mut v = Vec::new();
    for _ in 0..n {
        let l = r.u32().ok()? as usize;
        let key = r.take(l).ok()?.to_vec();
        let init_off = r.u64().ok()?;
        let status = r.u8().ok()?;
        let opt = |r: &mut Rd| -> Option<Option<Val>> {
            match r.u8().ok()? {
                0 => Some(None),
                _ => Some(Some(r.val().ok()?)),
            }
        };
        let exports = opt(&mut r)?;
        let factory = opt(&mut r)?;
        v.push(RegRec {
            key,
            init_off,
            status,
            exports,
            factory,
        });
    }
    Some((v, r.1))
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
