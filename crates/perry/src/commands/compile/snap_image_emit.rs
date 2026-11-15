//! Image-run builds: put the captured heap into the binary.
//!
//! With `PERRY_SNAP_IMAGE_RUN` set while compiling, the driver links a first
//! binary (B0), runs it up to the end of the static dependency inits (it
//! writes the image file and exits), names every changed root word by B0's
//! symbol table, and links a second binary (B1) that carries the cells in the
//! section `perry_snap_image`. The section sits at a fixed address far above
//! B0's `_end`, so no other section of B1 moves: every address the file
//! records (`__data_start`, `__executable_start`, TLS offsets) is the same in
//! B1. B1 does not use the section yet.
//!
//! The file format is the runtime's own, included unchanged.

#[allow(dead_code)]
#[path = "../../../../perry-runtime/src/snap_image/format.rs"]
mod format;
#[allow(dead_code)]
#[path = "../../../../perry-runtime/src/snap_image/format_read.rs"]
mod format_read;

use format::{Cell, Image, Val, Word};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SECTION: &str = "perry_snap_image";
const SECTION_MAGIC: u64 = 0x3130_4742_4d49_5350; // "PSIMBG01"
const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const CELL_HEADER: u64 = 8;

#[derive(Clone, Debug)]
struct Sym {
    addr: u64,
    size: u64,
    name: String,
}

#[derive(Clone, Debug, Default)]
struct Symtab {
    mem: Vec<Sym>,
    tls: Vec<Sym>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Sec {
    name: String,
    addr: u64,
    size: u64,
}

fn num(s: &str) -> Option<u64> {
    match s.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

fn parse_syms(text: &str) -> Symtab {
    let mut t = Symtab::default();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 8 || !f[0].ends_with(':') {
            continue;
        }
        let (Some(addr), Some(size)) = (u64::from_str_radix(f[1], 16).ok(), num(f[2])) else {
            continue;
        };
        let s = Sym {
            addr,
            size,
            name: f[7].to_string(),
        };
        match f[3] {
            "TLS" => t.tls.push(s),
            "OBJECT" | "FUNC" | "NOTYPE" if addr != 0 => t.mem.push(s),
            _ => {}
        }
    }
    t.mem.sort_by_key(|s| s.addr);
    t.tls.sort_by_key(|s| s.addr);
    t
}

fn parse_secs(text: &str) -> Vec<Sec> {
    let mut v = Vec::new();
    for line in text.lines() {
        let Some(i) = line.find(']') else { continue };
        if !line.trim_start().starts_with('[') {
            continue;
        }
        let f: Vec<&str> = line[i + 1..].split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        let (Some(addr), Some(size)) = (
            u64::from_str_radix(f[2], 16).ok(),
            u64::from_str_radix(f[4], 16).ok(),
        ) else {
            continue;
        };
        if addr != 0 {
            v.push(Sec {
                name: f[0].to_string(),
                addr,
                size,
            });
        }
    }
    v
}

fn find_sym(t: &Symtab, name: &str) -> Option<u64> {
    t.mem.iter().find(|s| s.name == name).map(|s| s.addr)
}

/// The symbol that covers `a`.
fn sym_at(v: &[Sym], a: u64) -> Option<&Sym> {
    let i = v.partition_point(|s| s.addr <= a);
    v[..i]
        .iter()
        .rev()
        .take(16)
        .find(|s| a < s.addr.saturating_add(s.size.max(1)))
}

/// Statics that stay as the runtime start-up leaves them: the allocator's
/// internals and the runtime's caches (each re-validated on use) and thread
/// identity. A changed word inside one is not imaged.
const PRISTINE: &[&str] = &[
    "heap_main",
    "theap_main",
    "mi_options",
    "mimalloc",
    "LEARNED_INLINE_FIELDS",
    "PRIVATE_NAME_MEMO",
    "PRIVATE_NAME_STORAGE",
    "PRIVATE_NAME_PROOF",
    "READ_STUB",
    "KEY_PREFIX_CACHE",
    "CONCAT_MEMO",
    "TYPEOF_CACHE",
    "REGEX_CACHE",
    "CANONICAL_KEYS",
    "birth_memo",
    "THREAD_TOKEN",
    "CURRENT_THREAD_ID",
    "perry_ic",
    // Allocator internals (its per-process sub-heap records).
    "subproc",
    // Process state the start-up of every run derives again: clock origins,
    // entry path, one-shot installs, the policy and feature caches read from
    // the environment, the lazily built stack maps, the arena reservation
    // (ASLR), the hash seeds and the random-source entry.
    "PERF_CLOCK",
    "process_epoch_us",
    "PROCESS_ENTRY_PATH",
    "ignore_sigpipe_at_startup",
    "maybe_install_stats_hook",
    "allocator_purge_enabled",
    "gc_scavenge_nursery_cap_bytes",
    "lazy_stack_maps_enabled",
    "STACK_MAPS",
    "reservation",
    "CLAIM_LOCK",
    "get_fixed_seeds",
    "GETRANDOM_FN",
    "5timer5store5STORE",
    "PRIMARY_IMAGE",
];

fn pristine(name: &str) -> bool {
    name.starts_with("mi_") || name.starts_with("_mi_") || PRISTINE.iter().any(|p| name.contains(p))
}

/// The symbols that cover some byte of the word at `a`.
fn overlapping(v: &[Sym], a: u64) -> Vec<&Sym> {
    let i = v.partition_point(|s| s.addr < a + 8);
    v[..i]
        .iter()
        .rev()
        .take(32)
        .filter(|s| s.size > 0 && s.addr + s.size > a)
        .collect()
}

/// Per-module state flags the code generator emits as single bytes: a word
/// that covers only these holds flag bytes, never an address.
fn flag_sym(n: &str) -> bool {
    n.starts_with("__perry_init_done_")
        || n.starts_with("__perry_agent_strings_ready_")
        || n.starts_with("__perry_literals_ready_")
        || n == "completed.0"
        || n == "PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED"
}

struct Classified {
    kept: Vec<Word>,
    dropped: usize,
    unresolved: Vec<String>,
}

/// Name every changed root word. A word in a pristine static is dropped; a
/// word no symbol covers, or one that holds a heap or stack address outside
/// a pristine static, is unresolved.
fn classify(words: &[Word], st: &Symtab, data_start: u64) -> Classified {
    let mut c = Classified {
        kept: Vec::new(),
        dropped: 0,
        unresolved: Vec::new(),
    };
    for w in words {
        let tls = w.region == format::WORD_TLS;
        let (kind, a) = if tls {
            ("tls", w.off)
        } else {
            ("data", data_start.wrapping_add(w.off))
        };
        let Some(s) = sym_at(if tls { &st.tls } else { &st.mem }, a) else {
            c.unresolved
                .push(format!("{kind}+{:#x}: no symbol names it", w.off));
            continue;
        };
        if pristine(&s.name) {
            c.dropped += 1;
        } else if let Val::Opaque(x) = w.val {
            let ov = if tls { Vec::new() } else { overlapping(&st.mem, a) };
            if !ov.is_empty() && ov.iter().all(|o| flag_sym(&o.name)) {
                c.kept.push(Word {
                    val: Val::Scalar(x),
                    ..w.clone()
                });
                continue;
            }
            c.unresolved.push(format!(
                "{kind}+{:#x} ({}+{:#x}): holds {x:#x}, a heap or stack address, and no pristine form covers it",
                w.off,
                s.name,
                a - s.addr
            ));
        } else {
            c.kept.push(w.clone());
        }
    }
    c
}

fn bytes_ll(b: &[u8]) -> String {
    let mut s = format!("[{} x i8] c\"", b.len());
    for x in b {
        s += &format!("\\{x:02X}");
    }
    s.push('"');
    s
}

/// The constant a slot or word holds in the binary: a number, or the address
/// of a cell or of a place in a named region plus the NaN-box tag.
fn val_ll(v: &Val, ncells: usize) -> Result<(u64, String), String> {
    let add = |hi: u16, off: u64| (off.wrapping_add(u64::from(hi) << 48)) as i64;
    let at = |sym: &str, a: i64| {
        format!("ptrtoint (ptr getelementptr (i8, ptr @{sym}, i64 {a}) to i64)")
    };
    Ok(match v {
        Val::Scalar(x) => (0, format!("{}", *x as i64)),
        Val::Cell { hi, index, .. } => {
            if *index as usize >= ncells {
                return Err(format!("a value names cell {index} of {ncells}"));
            }
            (
                1,
                at(
                    &format!("perry_snap_cell_{index}"),
                    add(*hi, CELL_HEADER),
                ),
            )
        }
        Val::Ext { hi, region, off, .. } => {
            let sym = match *region {
                format::REGION_STRINGS => "__start_perry_strings",
                format::REGION_LAYOUTS => "__start_perry_layouts",
                format::REGION_ATOMS => "__start_perry_atoms",
                format::REGION_EXE => "__executable_start",
                r => return Err(format!("a value points into region {r}, which has no address")),
            };
            (2 | (u64::from(*region) << 8), at(sym, add(*hi, *off)))
        }
        Val::Opaque(x) => {
            return Err(format!("a value holds {x:#x}, a heap or stack address"));
        }
    })
}

/// The LLVM IR of the image section: header, cell table, slot table, word
/// table, then every cell with its slot words as link-time constants.
fn image_ll(img: &Image, words: &[Word], data_start: u64) -> Result<String, String> {
    let nc = img.cells.len();
    let mut cells = String::new();
    let mut slot_rows: Vec<String> = Vec::new();
    let mut used: Vec<String> = vec!["@perry_snap_image".into()];
    for (ci, c) in img.cells.iter().enumerate() {
        let mut order: Vec<&(u32, Val)> = c.slots.iter().collect();
        order.sort_by_key(|s| s.0);
        let (mut ty, mut val) = (Vec::new(), Vec::new());
        let mut pos = 0usize;
        for (off, v) in order {
            let o = *off as usize;
            if o < pos || o + 8 > c.bytes.len() {
                return Err(format!("cell {ci}: slot at {o} is outside or overlaps"));
            }
            if o > pos {
                ty.push(format!("[{} x i8]", o - pos));
                val.push(bytes_ll(&c.bytes[pos..o]));
            }
            let (_, e) = val_ll(v, nc).map_err(|m| format!("cell {ci} (type {}) slot {o}: {m}", c.obj_type))?;
            ty.push("i64".to_string());
            val.push(format!("i64 {e}"));
            pos = o + 8;
        }
        if pos < c.bytes.len() {
            ty.push(format!("[{} x i8]", c.bytes.len() - pos));
            val.push(bytes_ll(&c.bytes[pos..]));
        }
        cells += &format!(
            "@perry_snap_cell_{ci} = global <{{ {} }}> <{{ {} }}>, section \"{SECTION}\", align 8\n",
            ty.join(", "),
            val.join(", ")
        );
        used.push(format!("@perry_snap_cell_{ci}"));
        for (off, v) in &c.slots {
            let (k, _) = val_ll(v, nc)?;
            slot_rows.push(format!("[3 x i64] [i64 {ci}, i64 {off}, i64 {k}]"));
        }
    }
    let table = |name: &str, n: usize, w: usize, rows: &[String]| {
        if n == 0 {
            format!("@{name} = global [0 x [{w} x i64]] zeroinitializer, section \"{SECTION}\", align 8\n")
        } else {
            format!(
                "@{name} = global [{n} x [{w} x i64]] [{}], section \"{SECTION}\", align 8\n",
                rows.join(", ")
            )
        }
    };
    let cell_rows: Vec<String> = img
        .cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let meta = u64::from(c.obj_type)
                | u64::from(c.flags) << 8
                | u64::from(c.reserved) << 16
                | u64::from(c.size) << 32;
            format!(
                "[2 x i64] [i64 ptrtoint (ptr @perry_snap_cell_{i} to i64), i64 {}]",
                meta as i64
            )
        })
        .collect();
    let mut word_rows = Vec::new();
    for w in words {
        let (k, e) = val_ll(&w.val, nc).map_err(|m| format!("word {:#x}: {m}", w.off))?;
        let addr = if w.region == format::WORD_DATA {
            data_start.wrapping_add(w.off)
        } else {
            w.off
        };
        word_rows.push(format!(
            "[4 x i64] [i64 {}, i64 {}, i64 {k}, i64 {e}]",
            w.region, addr as i64
        ));
    }
    let ptr = |n: &str| format!("i64 ptrtoint (ptr @{n} to i64)");
    let mut ll = String::from("; Perry image run: the captured heap as link-time data\n\n");
    ll += &format!(
        "@perry_snap_image = global <{{ i64, i64, i64, i64, i64, i64, i64, i64 }}> <{{ i64 {}, i64 {nc}, i64 {}, i64 {}, i64 {}, {}, {}, {} }}>, section \"{SECTION}\", align 8\n",
        SECTION_MAGIC as i64,
        slot_rows.len(),
        word_rows.len(),
        img.pending,
        ptr("perry_snap_cells"),
        ptr("perry_snap_slots"),
        ptr("perry_snap_words"),
    );
    ll += &table("perry_snap_cells", nc, 2, &cell_rows);
    ll += &table("perry_snap_slots", slot_rows.len(), 3, &slot_rows);
    ll += &table("perry_snap_words", word_rows.len(), 4, &word_rows);
    for n in ["perry_snap_cells", "perry_snap_slots", "perry_snap_words"] {
        used.push(format!("@{n}"));
    }
    ll += &cells;
    for n in [
        "__start_perry_strings",
        "__start_perry_layouts",
        "__start_perry_atoms",
        "__executable_start",
    ] {
        ll += &format!("@{n} = external global i8\n");
    }
    ll += &format!(
        "@llvm.used = appending global [{} x ptr] [{}], section \"llvm.metadata\"\n",
        used.len(),
        used.iter().map(|u| format!("ptr {u}")).collect::<Vec<_>>().join(", ")
    );
    Ok(ll)
}

struct Bases {
    strings: u64,
    layouts: u64,
    atoms: u64,
    exe: u64,
    data_start: u64,
}

/// Read the section back from the linked binary into an image.
fn decode_section(sec: &[u8], vaddr: u64, b: &Bases) -> Result<Image, String> {
    let rd = |o: u64| -> Result<u64, String> {
        let o = o as usize;
        sec.get(o..o + 8)
            .map(|s| u64::from_le_bytes(s.try_into().unwrap_or([0; 8])))
            .ok_or_else(|| format!("section read at {o} is outside the section"))
    };
    let at = |a: u64| -> Result<u64, String> {
        a.checked_sub(vaddr)
            .ok_or_else(|| format!("address {a:#x} is below the section"))
    };
    if rd(0)? != SECTION_MAGIC {
        return Err("the section does not start with the image header".into());
    }
    let (nc, ns, nw, pending) = (rd(8)?, rd(16)?, rd(24)?, rd(32)?);
    let (tc, ts, tw) = (at(rd(40)?)?, at(rd(48)?)?, at(rd(56)?)?);
    let mut img = Image {
        pending: pending as u32,
        ..Image::default()
    };
    let mut user: std::collections::HashMap<u64, u32> = Default::default();
    for i in 0..nc {
        let (a, meta) = (rd(tc + i * 16)?, rd(tc + i * 16 + 8)?);
        let size = (meta >> 32) as u32;
        let o = at(a)? as usize;
        let bytes = sec
            .get(o..o + size as usize)
            .ok_or("a cell is outside the section")?
            .to_vec();
        user.insert(a + CELL_HEADER, i as u32);
        img.cells.push(Cell {
            obj_type: meta as u8,
            flags: (meta >> 8) as u8,
            reserved: (meta >> 16) as u16,
            size,
            bytes,
            slots: Vec::new(),
        });
    }
    let unval = |info: u64, v: u64| -> Result<Val, String> {
        let (hi, a) = ((v >> 48) as u16, v & ADDR_MASK);
        Ok(match info & 0xff {
            0 => Val::Scalar(v),
            1 => Val::Cell {
                hi,
                bare: hi == 0,
                index: *user.get(&a).ok_or("a value names no cell")?,
            },
            2 => {
                let region = (info >> 8) as u8;
                let base = match region {
                    format::REGION_STRINGS => b.strings,
                    format::REGION_LAYOUTS => b.layouts,
                    format::REGION_ATOMS => b.atoms,
                    _ => b.exe,
                };
                Val::Ext {
                    hi,
                    bare: hi == 0,
                    region,
                    off: a.wrapping_sub(base),
                }
            }
            k => return Err(format!("bad value kind {k}")),
        })
    };
    for i in 0..ns {
        let (ci, off, info) = (
            rd(ts + i * 24)? as usize,
            rd(ts + i * 24 + 8)? as u32,
            rd(ts + i * 24 + 16)?,
        );
        let c = img.cells.get_mut(ci).ok_or("a slot names no cell")?;
        let o = off as usize;
        let raw = c
            .bytes
            .get(o..o + 8)
            .map(|s| u64::from_le_bytes(s.try_into().unwrap_or([0; 8])))
            .ok_or("a slot is outside its cell")?;
        let v = unval(info, raw)?;
        c.bytes[o..o + 8].fill(0);
        c.slots.push((off, v));
    }
    for i in 0..nw {
        let r = tw + i * 32;
        let (region, addr, info, v) = (rd(r)?, rd(r + 8)?, rd(r + 16)?, rd(r + 24)?);
        img.words.push(Word {
            region: region as u8,
            off: if region as u8 == format::WORD_DATA {
                addr.wrapping_sub(b.data_start)
            } else {
                addr
            },
            val: unval(info, v)?,
        });
    }
    Ok(img)
}

/// Every section of B0 keeps its address and size in B1.
fn proof(b0: &[Sec], b1: &[Sec]) -> Result<usize, String> {
    let mut n = 0;
    for s in b0 {
        match b1.iter().find(|t| t.name == s.name) {
            Some(t) if t.addr == s.addr && t.size == s.size => n += 1,
            Some(t) => {
                return Err(format!(
                    "section {} moved: {:#x}+{:#x} in B0, {:#x}+{:#x} in B1",
                    s.name, s.addr, s.size, t.addr, t.size
                ))
            }
            None => return Err(format!("section {} is missing in B1", s.name)),
        }
    }
    Ok(n)
}

fn tool(cmd: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if !o.status.success() {
        return Err(format!("{cmd} exited with {}", o.status));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

pub(super) struct Prepared {
    pub object: PathBuf,
    pub section_addr: u64,
    expected: Image,
    b0_secs: Vec<Sec>,
    b0_syms: [u64; 3],
    dropped: usize,
    total_words: usize,
    b0_dump: String,
}

fn probe(b: &Path) -> Result<(Symtab, Vec<Sec>), String> {
    let p = b.to_string_lossy();
    let st = parse_syms(&tool("readelf", &["-sW", &p])?);
    let secs = parse_secs(&tool("readelf", &["-SW", &p])?);
    Ok((st, secs))
}

/// Run B0 to the cut, name the words, build the image object.
pub(super) fn prepare(b0: &Path, work: &Path, target: Option<&str>) -> Result<Prepared, String> {
    let file = PathBuf::from(format!("{}.img", b0.display()));
    let _ = std::fs::remove_file(&file);
    let st = Command::new(b0)
        .env("PERRY_SNAP_IMAGE_OUT", &file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("the image run did not start: {e}"))?;
    if !st.success() {
        return Err(format!("the image run exited with {st}"));
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("no image file: {e}"))?;
    let img = format_read::decode(&bytes)?;
    if let Some(t) = img.stops.first() {
        return Err(format!(
            "{} cell(s) have no image form; first: cell {} type {} kind {} off {}",
            img.stops.len(),
            t.cell,
            t.obj_type,
            t.kind,
            t.off
        ));
    }
    let (syms, secs) = probe(b0)?;
    let need = |n: &str| find_sym(&syms, n).ok_or_else(|| format!("B0 has no symbol {n} (symbols stripped?)"));
    let (data_start, end, exe_start) = (need("__data_start")?, need("_end")?, need("__executable_start")?);
    let c = classify(&img.words, &syms, data_start);
    if !c.unresolved.is_empty() {
        let full = PathBuf::from(format!("{}.unresolved", b0.display()));
        let _ = std::fs::write(&full, c.unresolved.join("\n"));
        return Err(format!(
            "{} root word(s) have no name and no pristine form (listed in {}); first: {}",
            c.unresolved.len(),
            full.display(),
            c.unresolved[0]
        ));
    }
    let ll = image_ll(&img, &c.kept, data_start)?;
    let obj = perry_codegen::linker::compile_ll_to_object(&ll, target.and_then(perry_codegen::resolve_target_triple).as_deref())
        .map_err(|e| format!("image object: {e}"))?;
    let object = work.join("_perry_snap_image.o");
    std::fs::write(&object, obj).map_err(|e| format!("image object: {e}"))?;
    let section_addr = end.div_ceil(0x20_0000) * 0x20_0000 + 0x20_0000;
    let b0_dump = format_read::dump(&img);
    let total_words = img.words.len();
    let expected = Image {
        cells: img.cells,
        words: c.kept,
        stops: Vec::new(),
        pending: img.pending,
    };
    Ok(Prepared {
        object,
        section_addr,
        expected,
        b0_secs: secs.into_iter().filter(|s| s.name != SECTION).collect(),
        b0_syms: [data_start, end, exe_start],
        dropped: c.dropped,
        total_words,
        b0_dump,
    })
}

/// Check B1: nothing moved, the section is where it was put, and it decodes
/// to the image the dump holds. Returns the report text.
pub(super) fn verify(b1: &Path, p: &Prepared) -> Result<String, String> {
    let (st, secs) = probe(b1)?;
    let n = proof(&p.b0_secs, &secs)?;
    let sym = |n: &str| find_sym(&st, n).unwrap_or(0);
    let now = [sym("__data_start"), sym("_end"), sym("__executable_start")];
    if now != p.b0_syms {
        return Err(format!("B1 moved a boundary symbol: {:x?} in B0, {now:x?} in B1", p.b0_syms));
    }
    let s = secs
        .iter()
        .find(|s| s.name == SECTION)
        .ok_or("B1 has no perry_snap_image section")?;
    if s.addr != p.section_addr {
        return Err(format!("the section is at {:#x}, wanted {:#x}", s.addr, p.section_addr));
    }
    let bin = PathBuf::from(format!("{}.sec", b1.display()));
    tool(
        "objcopy",
        &[
            "-O",
            "binary",
            &format!("--only-section={SECTION}"),
            &b1.to_string_lossy(),
            &bin.to_string_lossy(),
        ],
    )?;
    let raw = std::fs::read(&bin).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&bin);
    let bases = Bases {
        strings: sym("__start_perry_strings"),
        layouts: sym("__start_perry_layouts"),
        atoms: sym("__start_perry_atoms"),
        exe: p.b0_syms[2],
        data_start: p.b0_syms[0],
    };
    let got = decode_section(&raw, s.addr, &bases)?;
    if got != p.expected {
        return Err(format!(
            "the section does not decode to the image: cells {} vs {}, words {} vs {}",
            got.cells.len(),
            p.expected.cells.len(),
            got.words.len(),
            p.expected.words.len()
        ));
    }
    Ok(format!(
        "proof sections-equal={n} boundary-symbols-equal=yes section-at={:#x}\ndecoded-equals-image=yes\nwords total={} dropped-pristine={} kept={}\n-- B0 dump --\n{}-- section dump --\n{}",
        s.addr,
        p.total_words,
        p.dropped,
        got.words.len(),
        p.b0_dump,
        format_read::dump(&got)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYMS: &str = "Symbol table '.symtab' contains 4 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n    10: 0000000000600000     8 OBJECT  LOCAL  DEFAULT   24 _mi_heap_main\n    11: 0000000000600010    16 OBJECT  GLOBAL DEFAULT   24 perry_global_x\n    12: 0000000000000040     8 TLS     LOCAL  DEFAULT   20 perry_tls_y\n    13: 0000000000600020     8 OBJECT  LOCAL  DEFAULT   24 SOME_CACHE\n";

    fn word(region: u8, off: u64, val: Val) -> Word {
        Word { region, off, val }
    }

    #[test]
    fn symbols_and_sections_are_read() {
        let t = parse_syms(SYMS);
        assert_eq!(t.mem.len(), 3);
        assert_eq!(t.tls.len(), 1);
        assert_eq!(sym_at(&t.mem, 0x600014).map(|s| s.name.as_str()), Some("perry_global_x"));
        assert!(sym_at(&t.mem, 0x600008).is_none());
        let s = parse_secs("  [ 0]                   NULL            0000000000000000 000000 000000 00      0   0  0\n  [16] .tdata            PROGBITS        0000000000600000 001000 000010 00 WAT  0   0  8\n");
        assert_eq!(s, vec![Sec { name: ".tdata".into(), addr: 0x600000, size: 0x10 }]);
    }

    #[test]
    fn words_are_named_or_stop() {
        let t = parse_syms(SYMS);
        let ws = [
            word(format::WORD_DATA, 0, Val::Scalar(1)),
            word(format::WORD_DATA, 0x10, Val::Scalar(2)),
            word(format::WORD_DATA, 0x100, Val::Scalar(3)),
            word(format::WORD_DATA, 0x20, Val::Opaque(0x7f00_0000_1000)),
            word(format::WORD_TLS, 0x40, Val::Scalar(4)),
        ];
        let c = classify(&ws, &t, 0x600000);
        assert_eq!(c.dropped, 1, "the allocator static is pristine");
        assert_eq!(c.kept.len(), 2, "named scalar words stay");
        assert_eq!(c.unresolved.len(), 2, "{:?}", c.unresolved);
        assert!(c.unresolved[0].contains("no symbol names it"));
        assert!(c.unresolved[1].contains("SOME_CACHE"));
    }

    fn cell(slots: Vec<(u32, Val)>) -> Cell {
        Cell { obj_type: 2, flags: 0x15, reserved: 0, size: 24, bytes: vec![7; 24], slots }
    }

    #[test]
    fn init_flag_bytes_are_scalar_words() {
        let t = parse_syms("    1: 0000000000600030     1 OBJECT  LOCAL  DEFAULT   24 __perry_init_done_m\n    2: 0000000000600031     1 OBJECT  LOCAL  DEFAULT   24 __perry_literals_ready_m\n    3: 0000000000600038     8 OBJECT  LOCAL  DEFAULT   24 OTHER\n");
        let ws = [
            word(format::WORD_DATA, 0x30, Val::Opaque(0x0101_0100)),
            word(format::WORD_DATA, 0x38, Val::Opaque(0x0101_0100)),
        ];
        let c = classify(&ws, &t, 0x600000);
        assert_eq!(c.kept.len(), 1, "{:?}", c.unresolved);
        assert_eq!(c.kept[0].val, Val::Scalar(0x0101_0100));
        assert_eq!(c.unresolved.len(), 1, "a word of another static is not a flag word");
    }

    #[test]
    fn cells_become_link_time_constants() {
        let tag = Val::Cell { hi: 0x7FFD, bare: false, index: 1 };
        let img = Image {
            cells: vec![cell(vec![(8, tag)]), cell(vec![(16, Val::Ext { hi: 0, bare: true, region: format::REGION_EXE, off: 0x40 })])],
            ..Image::default()
        };
        let ll = image_ll(&img, &[], 0).expect("ll");
        assert!(ll.contains(&format!("@perry_snap_cell_1, i64 {})", ((0x7FFDu64 << 48) + 8) as i64)), "{ll}");
        assert!(ll.contains("ptr @__executable_start, i64 64"), "{ll}");
        assert!(ll.contains("section \"perry_snap_image\""));
        let bad = Image { cells: vec![cell(vec![(8, Val::Opaque(0x7f00_0000_2000))])], ..Image::default() };
        assert!(image_ll(&bad, &[], 0).is_err());
        let idx = Image { cells: vec![cell(vec![(8, Val::Cell { hi: 0, bare: true, index: 5 })])], ..Image::default() };
        assert!(image_ll(&idx, &[], 0).is_err());
    }

    #[test]
    fn the_section_decodes_to_its_image() {
        let v = 0x1000_0000u64;
        let mut s = vec![0u8; 168];
        let mut put = |o: usize, x: u64| s[o..o + 8].copy_from_slice(&x.to_le_bytes());
        for (o, x) in [(0, SECTION_MAGIC), (8, 1), (16, 1), (24, 1), (32, 7), (40, v + 64), (48, v + 80), (56, v + 104)] {
            put(o, x);
        }
        put(64, v + 136);
        put(72, 2 | 0x15 << 8 | 24 << 32);
        put(80, 0);
        put(88, 8);
        put(96, 1);
        for (o, x) in [(104, 0), (112, 0x600010), (120, 0), (128, 5)] {
            put(o, x);
        }
        put(144, (0x7FFDu64 << 48) | (v + 136 + 8));
        let b = Bases { strings: 0, layouts: 0, atoms: 0, exe: 0x400000, data_start: 0x600000 };
        let got = decode_section(&s, v, &b).expect("decode");
        let mut bytes = vec![0u8; 24];
        bytes[0] = 0;
        let want = Image {
            cells: vec![Cell { obj_type: 2, flags: 0x15, reserved: 0, size: 24, bytes, slots: vec![(8, Val::Cell { hi: 0x7FFD, bare: false, index: 0 })] }],
            words: vec![word(format::WORD_DATA, 0x10, Val::Scalar(5))],
            stops: vec![],
            pending: 7,
        };
        assert_eq!(got, want);
    }

    #[test]
    fn a_moved_section_fails_the_proof() {
        let a = vec![Sec { name: ".data".into(), addr: 0x600000, size: 16 }, Sec { name: ".bss".into(), addr: 0x600010, size: 8 }];
        assert_eq!(proof(&a, &a), Ok(2));
        let mut b = a.clone();
        b[1].addr += 0x1000;
        assert!(proof(&a, &b).is_err());
        let mut c = a.clone();
        c[0].size += 8;
        assert!(proof(&a, &c).is_err());
        assert!(proof(&a, &a[..1]).is_err());
    }
}
