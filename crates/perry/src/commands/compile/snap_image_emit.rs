//! Image-run builds: put the captured heap into the binary.
//!
//! With `PERRY_SNAP_IMAGE_RUN` set while compiling, the driver links a first
//! binary (B0), runs it up to the end of the static dependency inits (it
//! writes the image file and exits), names every changed root word by B0's
//! symbol table, and links a second binary (B1) that carries the cells in the
//! section `perry_snap_image`. B0 is linked with a one-word placeholder in the
//! same section at the same fixed address, so both links have the same program
//! header table and no other section moves: every address the file records
//! (`__data_start`, `__executable_start`, TLS offsets) is the same in B1. The
//! proof lists every section whose address or size differs. B1 does not use
//! the section yet.
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
/// Where the image section is linked, in B0 (placeholder) and in B1.
///
/// Address rule: the section needs a fixed address that is known before B0 is
/// linked, so it cannot follow B0's `_end`. It is 1 GiB: below 2 GiB (the
/// non-PIE small code model reaches it), far above the load address (4 MiB),
/// and `prepare` demands `_end + IMAGE_HEADROOM <= IMAGE_ADDR`, so the bss, the
/// brk heap that starts after it and the reservations made at startup cannot
/// reach it. The window `[IMAGE_ADDR, IMAGE_ADDR + IMAGE_WINDOW)` is what the
/// image may occupy; a larger image, or a B0 whose `_end` is too high, is "no
/// image", never an unproven link.
pub(super) const IMAGE_ADDR: u64 = 0x4000_0000;
const IMAGE_WINDOW: u64 = 0x1000_0000;
const IMAGE_HEADROOM: u64 = 0x1000_0000;

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

/// The exact path of a symbol: a Rust name demangled without its hash and
/// without the linker's local-copy suffix; a C name as it is.
fn sym_path(name: &str) -> String {
    let base = name.split('.').next().unwrap_or(name);
    format!("{:#}", rustc_demangle::demangle(base))
}

/// `path` is `p` or lies under it (whole path components, never a substring).
fn under(path: &str, p: &str) -> bool {
    path.strip_prefix(p)
        .is_some_and(|r| r.is_empty() || r.starts_with("::"))
}

/// Statics that stay as the runtime start-up leaves them, by exact path: the
/// allocator's internals and the runtime's caches (each re-validated on use),
/// process state every run derives again (clock origins, entry path, one-shot
/// installs, policy caches read from the environment, the lazily built stack
/// maps, the arena reservation, hash seeds, the random-source entry). A
/// changed word inside one is not imaged.
const PRISTINE: &[&str] = &[
    "heap_main",
    "theap_main",
    "__mi_theap_main",
    "mi_options",
    "subproc_main",
    "subprocs_lock",
    "subprocs",
    "perry_runtime::object::spill::LEARNED_INLINE_FIELDS",
    "perry_runtime::object::field_get_set::ic_miss::PRIVATE_NAME_MEMO",
    "perry_runtime::object::read_stub::READ_STUB",
    "perry_runtime::json::stringify_record_output::KEY_PREFIX_CACHE",
    "perry_runtime::string::concat::CONCAT_MEMO",
    "perry_runtime::string::concat::CONCAT_MEMO_TAGS",
    "perry_runtime::builtins::arithmetic::TYPEOF_CACHE",
    "perry_runtime::regex::perex_cache::REGEX_CACHE",
    "perry_runtime::object::canonical_keys::CANONICAL_KEYS",
    "perry_runtime::node_stream::constructors::birth_memo",
    "perry_runtime::native_handle::current_thread_id::CURRENT_THREAD_ID",
    "perry_runtime::perf_clock::PERF_CLOCK",
    "perry_runtime::gc::instruments::process_epoch_us",
    "perry_runtime::os::PROCESS_ENTRY_PATH",
    "perry_runtime::process_startup::ignore_sigpipe_at_startup",
    "perry_runtime::tls_hot::maybe_install_stats_hook",
    "perry_runtime::gc::cycle_malloc_trim::allocator_purge_enabled",
    "perry_runtime::gc::policy::gc_scavenge_nursery_cap_bytes",
    "perry_runtime::gc::roots::stack_maps::lazy_stack_maps_enabled",
    "perry_runtime::gc::roots::stack_maps::STACK_MAPS",
    "perry_runtime::gc::roots::stack_maps::STACK_MAPS_INITIALIZED",
    "perry_runtime::gc::roots::stack_maps::STACK_MAPS_REBUILD_OWED",
    "perry_runtime::arena::region::reservation::RESERVATION",
    "perry_runtime::arena::region::reservation::INITIAL",
    "perry_runtime::arena::region::reservation::SERIAL",
    "perry_runtime::timer::store::STORE",
    "ahash::random_state::get_fixed_seeds",
    "getrandom::backends::linux_android_with_fallback",
];

/// The published counters of the primary agent's timer store: nonzero means a
/// timer or check callback is pending.
const TIMER_COUNTERS: &[&str] = &[
    "perry_runtime::timer::store::PRIMARY_REFED_TIMERS",
    "perry_runtime::timer::store::PRIMARY_REFED_CHECK",
    "perry_runtime::timer::store::PRIMARY_CHECK_PENDING",
    "perry_runtime::timer::store::PRIMARY_ANY_PENDING",
];

/// What a changed word of a static means for the image.
#[derive(Debug, PartialEq)]
enum Judge {
    /// The start-up of B1 leaves it as it was: not imaged.
    Pristine,
    /// Imaged by value (or by its own form).
    Keep,
    /// No image: the reason.
    Stop(&'static str),
}

fn judge(name: &str, v: &Val) -> Judge {
    let p = sym_path(name);
    let is = |q: &str| under(&p, q);
    if is("perry_runtime::object::class_image::PRIMARY_IMAGE") {
        Judge::Stop("the class tables are filled by module init and the image has no form that restores them")
    } else if is("perry_runtime::tls_hot::CLAIM_LOCK") {
        Judge::Stop("the hot-slot claims are indices the imaged declarations cache, and the image has no form that restores the claim table")
    } else if TIMER_COUNTERS.iter().any(|q| is(q)) && *v != Val::Scalar(0) {
        Judge::Stop("a timer or check callback is pending at the cut")
    } else if is("perry_runtime::object::field_get_set::class_object_template::NEXT_THREAD_TOKEN") {
        // The primary claims token 1 in the image run and again in B1: sound
        // only while that was the one claim before the cut.
        if *v == Val::Scalar(2) {
            Judge::Pristine
        } else {
            Judge::Stop("more than one thread claimed a class-template token before the cut")
        }
    } else if name.starts_with("perry_concat_site_") {
        concat_site_word(v)
    } else if name.starts_with("mi_") || name.starts_with("_mi_") || PRISTINE.iter().any(|q| is(q))
    {
        Judge::Pristine
    } else {
        Judge::Keep
    }
}

struct Classified {
    kept: Vec<Word>,
    dropped: usize,
    dropped_by: std::collections::BTreeMap<String, usize>,
    unresolved: Vec<String>,
}

/// Name every changed root word. A word in a pristine static is dropped; a
/// word no symbol covers, one a static's rule stops, or one that holds a heap,
/// stack or symbol-space address outside a pristine static, is unresolved.
#[cfg(test)]
fn classify(words: &[Word], st: &Symtab, data_start: u64) -> Classified {
    classify_with(words, st, data_start, false)
}

/// Whether a path names a static the module-path replay record covers.
fn covered_by_record(path: &str) -> bool {
    path.split("::")
        .any(|s| s == "MODULE_PATH_REGISTRY" || s == "PATH_MODULE_INIT_ADDRS")
}

/// One static as the judge sees it: every changed word of one symbol.
struct Unit {
    path: String,
    size: u64,
    kind: &'static str,
    first: u64,
    pristine: Vec<Word>,
    bad: Vec<String>,
    kept: Vec<Word>,
}

/// `classify`; with `record` the statics the replay record covers are dropped.
/// The unit of judgement is the static (one symbol), never the word: a static
/// with a stopped or unresolved word stops as a whole, a static with a
/// pristine word is pristine as a whole, and the scalar words of a runtime
/// container static (lengths, bump pointers, table sizes, slot indices) are
/// never kept by value on their own.
fn classify_with(words: &[Word], st: &Symtab, data_start: u64, record: bool) -> Classified {
    let mut c = Classified {
        kept: Vec::new(),
        dropped: 0,
        dropped_by: Default::default(),
        unresolved: Vec::new(),
    };
    let mut units: Vec<Unit> = Vec::new();
    let mut index: std::collections::BTreeMap<(bool, u64), usize> = Default::default();
    for w in words {
        let tls = w.region & format::WORD_TLS != 0;
        let (kind, a) = if tls {
            ("tls", w.off)
        } else {
            ("data", data_start.wrapping_add(w.off))
        };
        let Some(s) = sym_at(if tls { &st.tls } else { &st.mem }, a) else {
            c.unresolved.push(if tls {
                format!(
                    "{kind}+{:#x}: unnamed TLS word (outside every named TLS symbol)",
                    w.off
                )
            } else {
                format!("{kind}+{:#x}: no symbol names it", w.off)
            });
            continue;
        };
        let path = sym_path(&s.name);
        let at = format!("{kind}+{:#x} ({}+{:#x})", w.off, path, a - s.addr);
        let j = if record && covered_by_record(&path) {
            Judge::Pristine
        } else {
            judge(&s.name, &w.val)
        };
        let at_unit = *index.entry((tls, s.addr)).or_insert_with(|| {
            units.push(Unit {
                path: path.clone(),
                size: s.size,
                kind,
                first: w.off,
                pristine: Vec::new(),
                bad: Vec::new(),
                kept: Vec::new(),
            });
            units.len() - 1
        });
        let Some(u) = units.get_mut(at_unit) else {
            continue;
        };
        match j {
            Judge::Pristine => u.pristine.push(w.clone()),
            Judge::Stop(why) => u.bad.push(format!("{at}: {why}")),
            Judge::Keep => match w.val {
                Val::Opaque(x) => u.bad.push(format!(
                    "{at}: holds {x:#x}, a heap or stack address, and no pristine form covers it"
                )),
                Val::Sym(x) => u.bad.push(format!(
                    "{at}: holds {x:#x}, a symbol-space address, and the symbol space has no image form"
                )),
                _ => u.kept.push(w.clone()),
            },
        }
    }
    for u in units {
        if !u.bad.is_empty() {
            c.unresolved.extend(u.bad);
            if !u.kept.is_empty() {
                c.unresolved.push(format!(
                    "{}+{:#x} ({}): {} scalar word(s) are not kept by value; the static stops as one unit",
                    u.kind,
                    u.first,
                    u.path,
                    u.kept.len()
                ));
            }
        } else if !u.pristine.is_empty() {
            let n = u.pristine.len() + u.kept.len();
            c.dropped += n;
            *c.dropped_by.entry(u.path).or_default() += n;
        } else if runtime_container(&u.path, u.size) {
            c.unresolved.push(format!(
                "{}+{:#x} ({}): {} scalar word(s) of a runtime container static; no declaration says they may be kept by value",
                u.kind,
                u.first,
                u.path,
                u.kept.len()
            ));
        } else {
            c.kept.extend(u.kept);
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
                at(&format!("perry_snap_cell_{index}"), add(*hi, CELL_HEADER)),
            )
        }
        Val::Ext {
            hi, region, off, ..
        } => {
            let sym = match *region {
                format::REGION_STRINGS => "__start_perry_strings",
                format::REGION_LAYOUTS => "__start_perry_layouts",
                format::REGION_ATOMS => "__start_perry_atoms",
                format::REGION_EXE => "__executable_start",
                r => {
                    return Err(format!(
                        "a value points into region {r}, which has no address"
                    ))
                }
            };
            (2 | (u64::from(*region) << 8), at(sym, add(*hi, *off)))
        }
        Val::Opaque(x) => {
            return Err(format!("a value holds {x:#x}, a heap or stack address"));
        }
        Val::Sym(x) => {
            return Err(format!("a value holds {x:#x}, a symbol-space address"));
        }
        Val::Derived(k) => (5 | (u64::from(*k) << 8), "0".to_string()),
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
            let (_, e) = val_ll(v, nc)
                .map_err(|m| format!("cell {ci} (type {}) slot {o}: {m}", c.obj_type))?;
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
    for r in &img.registry {
        for v in [&r.exports, &r.factory].into_iter().flatten() {
            val_ll(v, nc).map_err(|m| format!("registry entry: {m}"))?;
        }
    }
    let ptr = |n: &str| format!("i64 ptrtoint (ptr @{n} to i64)");
    let mut ll = String::from("; Perry image run: the captured heap as link-time data\n\n");
    ll += &format!(
        "@perry_snap_image = global <{{ i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64 }}> <{{ i64 {}, i64 {nc}, i64 {}, i64 {}, i64 {}, {}, {}, {}, {}, i64 {}, {} }}>, section \"{SECTION}\", align 8\n",
        SECTION_MAGIC as i64,
        slot_rows.len(),
        word_rows.len(),
        img.pending,
        ptr("perry_snap_cells"),
        ptr("perry_snap_slots"),
        ptr("perry_snap_words"),
        ptr("perry_snap_claims"),
        img.claims_from,
        ptr("perry_snap_registry"),
    );
    ll += &format!(
        "@perry_snap_registry = global {}, section \"{SECTION}\", align 8\n",
        bytes_ll(&format::registry_blob(&img.registry))
    );
    ll += &format!(
        "@perry_snap_claims = global {}, section \"{SECTION}\", align 8\n",
        bytes_ll(&format::claims_blob(&img.claims))
    );
    ll += &table("perry_snap_cells", nc, 2, &cell_rows);
    ll += &table("perry_snap_slots", slot_rows.len(), 3, &slot_rows);
    ll += &table("perry_snap_words", word_rows.len(), 4, &word_rows);
    for n in [
        "perry_snap_cells",
        "perry_snap_slots",
        "perry_snap_words",
        "perry_snap_claims",
        "perry_snap_registry",
    ] {
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
        used.iter()
            .map(|u| format!("ptr {u}"))
            .collect::<Vec<_>>()
            .join(", ")
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
    let (cp, cf) = (at(rd(64)?)? as usize, rd(72)?);
    let rp = at(rd(80)?)? as usize;
    let mut img = Image {
        pending: pending as u32,
        claims_from: cf as u32,
        claims: format_read::parse_claims_blob(
            sec.get(cp..)
                .ok_or("the claim record is outside the section")?,
        )
        .ok_or("the claim record is malformed")?,
        registry: format_read::parse_registry_blob(
            sec.get(rp..)
                .ok_or("the registry record is outside the section")?,
        )
        .ok_or("the registry record is malformed")?
        .0,
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
            5 => Val::Derived((info >> 8) as u8),
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

/// Every section of B0 other than the image section keeps its address and
/// size in B1. All differences are reported, not the first.
fn proof(b0: &[Sec], b1: &[Sec]) -> Result<usize, String> {
    let mut n = 0;
    let mut bad = Vec::new();
    for s in b0.iter().filter(|s| s.name != SECTION) {
        match b1.iter().find(|t| t.name == s.name) {
            Some(t) if t.addr == s.addr && t.size == s.size => n += 1,
            Some(t) => bad.push(format!(
                "section {} moved: {:#x}+{:#x} in B0, {:#x}+{:#x} in B1",
                s.name, s.addr, s.size, t.addr, t.size
            )),
            None => bad.push(format!("section {} is missing in B1", s.name)),
        }
    }
    for t in b1.iter().filter(|t| t.name != SECTION) {
        if !b0.iter().any(|s| s.name == t.name) {
            bad.push(format!("section {} exists only in B1", t.name));
        }
    }
    if bad.is_empty() {
        Ok(n)
    } else {
        Err(format!(
            "{} section(s) differ: {}",
            bad.len(),
            bad.join("; ")
        ))
    }
}

/// The IR of B0's placeholder: one word in the image section.
fn placeholder_ll() -> String {
    format!(
        "@perry_snap_placeholder = global i64 1, section \"{SECTION}\", align 8\n@llvm.used = appending global [1 x ptr] [ptr @perry_snap_placeholder], section \"llvm.metadata\"\n"
    )
}

/// The object B0 is linked with, and the link argument that puts its section
/// at `IMAGE_ADDR`.
pub(super) fn placeholder(work: &Path, target: Option<&str>) -> Result<(PathBuf, String), String> {
    let obj = perry_codegen::linker::compile_ll_to_object(
        &placeholder_ll(),
        target
            .and_then(perry_codegen::resolve_target_triple)
            .as_deref(),
    )
    .map_err(|e| format!("placeholder object: {e}"))?;
    let path = work.join("_perry_snap_placeholder.o");
    std::fs::write(&path, obj).map_err(|e| format!("placeholder object: {e}"))?;
    Ok((
        path,
        format!("-Wl,--section-start={SECTION}={IMAGE_ADDR:#x}"),
    ))
}

/// The placeholder is in B0 where it must be and the bss cannot reach it.
fn check_b0_layout(secs: &[Sec], end: u64) -> Result<(), String> {
    match secs.iter().find(|s| s.name == SECTION) {
        Some(s) if s.addr == IMAGE_ADDR => {}
        Some(s) => {
            return Err(format!(
                "B0 has the placeholder at {:#x}, wanted {IMAGE_ADDR:#x}",
                s.addr
            ))
        }
        None => return Err("B0 has no placeholder section".into()),
    }
    if end + IMAGE_HEADROOM > IMAGE_ADDR {
        return Err(format!(
            "B0 ends at {end:#x}, too close to the image address {IMAGE_ADDR:#x} (headroom {IMAGE_HEADROOM:#x})"
        ));
    }
    Ok(())
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
    expected: Image,
    b0_secs: Vec<Sec>,
    b0_syms: [u64; 3],
    b0_phdrs: Vec<(u64, String)>,
    dropped: usize,
    dropped_by: String,
    total_words: usize,
    b0_dump: String,
}

fn probe(b: &Path) -> Result<(Symtab, Vec<Sec>, Vec<(u64, String)>), String> {
    let p = b.to_string_lossy();
    let st = parse_syms(&tool("readelf", &["-sW", &p])?);
    let secs = parse_secs(&tool("readelf", &["-SW", &p])?);
    let phs = parse_phdrs(&tool("readelf", &["-lW", &p])?);
    Ok((st, secs, phs))
}

/// The program header rows of `readelf -lW` as (virtual address, normalised row).
fn parse_phdrs(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut on = false;
    for l in text.lines() {
        if l.starts_with("Program Headers") {
            on = true;
        } else if on && l.trim().is_empty() {
            break;
        } else if on {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.get(1).is_some_and(|x| x.starts_with("0x")) {
                out.push((t.get(2).and_then(|x| num(x)).unwrap_or(0), t.join(" ")));
            }
        }
    }
    out
}

/// Every program header of B0 is B1's, apart from the image section's own
/// segment (its file contents and size differ by design).
fn phdr_proof(b0: &[(u64, String)], b1: &[(u64, String)]) -> Result<usize, String> {
    let rows = |v: &[(u64, String)]| -> Vec<String> {
        v.iter()
            .filter(|r| r.0 != IMAGE_ADDR)
            .map(|r| r.1.clone())
            .collect()
    };
    let (r0, r1) = (rows(b0), rows(b1));
    if !b0.iter().any(|r| r.0 == IMAGE_ADDR)
        || !b1.iter().any(|r| r.0 == IMAGE_ADDR)
        || r0.is_empty()
    {
        return Err("a program header table has no image segment or no rows".to_string());
    }
    if r0 != r1 {
        let d: Vec<String> = r0
            .iter()
            .filter(|r| !r1.contains(r))
            .map(|r| format!("B0 only: {r}"))
            .chain(
                r1.iter()
                    .filter(|r| !r0.contains(r))
                    .map(|r| format!("B1 only: {r}")),
            )
            .collect();
        return Err(format!("program headers differ: {}", d.join("; ")));
    }
    Ok(r0.len())
}

/// The list of unresolved words of one compile, beside B0.
fn unresolved_path(b0: &Path) -> PathBuf {
    PathBuf::from(format!("{}.unresolved", b0.display()))
}

/// Every compile writes its list fresh or leaves none.
fn clear_unresolved(b0: &Path) {
    let _ = std::fs::remove_file(unresolved_path(b0));
}

/// Run B0 to the cut, name the words, build the image object.
pub(super) fn prepare(b0: &Path, work: &Path, target: Option<&str>) -> Result<Prepared, String> {
    let file = PathBuf::from(format!("{}.img", b0.display()));
    let _ = std::fs::remove_file(&file);
    clear_unresolved(b0);
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
    if std::env::var_os("PERRY_SNAP_IMAGE_LIST_PAST_STOPS").is_some() {
        // Test only: name the words past a stop; the stop itself stays.
        if let Ok((syms, _, _)) = probe(b0) {
            if let Some(ds) = find_sym(&syms, "__data_start") {
                let c = classify_with(&img.words, &syms, ds, !img.registry.is_empty());
                if !c.unresolved.is_empty() {
                    let _ = std::fs::write(unresolved_path(b0), c.unresolved.join("\n"));
                }
            }
        }
    }
    if let Some(m) = class_table_message(&img.stops) {
        return Err(m);
    }
    if !img.stops.is_empty() {
        return Err(stop_summary(&img.stops));
    }
    let (syms, secs, phdrs) = probe(b0)?;
    let need = |n: &str| {
        find_sym(&syms, n).ok_or_else(|| format!("B0 has no symbol {n} (symbols stripped?)"))
    };
    let (data_start, end, exe_start) = (
        need("__data_start")?,
        need("_end")?,
        need("__executable_start")?,
    );
    check_b0_layout(&secs, end)?;
    let est: u64 = img
        .cells
        .iter()
        .map(|c| c.bytes.len() as u64 + 64 + 32 * c.slots.len() as u64)
        .sum::<u64>()
        + 64 * img.words.len() as u64
        + 4096;
    if est > IMAGE_WINDOW {
        return Err(format!(
            "the image needs about {est:#x} bytes, the window holds {IMAGE_WINDOW:#x}"
        ));
    }
    let c = classify_with(&img.words, &syms, data_start, !img.registry.is_empty());
    if !c.unresolved.is_empty() {
        let full = unresolved_path(b0);
        let _ = std::fs::write(&full, c.unresolved.join("\n"));
        return Err(format!(
            "{} root word(s) have no name and no pristine form (listed in {}); first: {}",
            c.unresolved.len(),
            full.display(),
            c.unresolved[0]
        ));
    }
    let ll = image_ll(&img, &c.kept, data_start)?;
    let obj = perry_codegen::linker::compile_ll_to_object(
        &ll,
        target
            .and_then(perry_codegen::resolve_target_triple)
            .as_deref(),
    )
    .map_err(|e| format!("image object: {e}"))?;
    let object = work.join("_perry_snap_image.o");
    std::fs::write(&object, obj).map_err(|e| format!("image object: {e}"))?;
    let b0_dump = format_read::dump(&img);
    let total_words = img.words.len();
    let expected = Image {
        cells: img.cells,
        words: c.kept,
        stops: Vec::new(),
        pending: img.pending,
        claims_from: img.claims_from,
        claims: img.claims,
        registry: img.registry,
        mint: Vec::new(),
        canon: None,
    };
    Ok(Prepared {
        object,
        expected,
        b0_secs: secs,
        b0_syms: [data_start, end, exe_start],
        b0_phdrs: phdrs,
        dropped: c.dropped,
        dropped_by: c
            .dropped_by
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; "),
        total_words,
        b0_dump,
    })
}

/// Check B1: nothing moved, the section is where it was put, and it decodes
/// to the image the dump holds. Returns the report text.
pub(super) fn verify(b1: &Path, p: &Prepared) -> Result<String, String> {
    let (st, secs, phs) = probe(b1)?;
    let n = proof(&p.b0_secs, &secs)?;
    let np = phdr_proof(&p.b0_phdrs, &phs)?;
    let sym = |n: &str| find_sym(&st, n).unwrap_or(0);
    let now = [sym("__data_start"), sym("_end"), sym("__executable_start")];
    if now != p.b0_syms {
        return Err(format!(
            "B1 moved a boundary symbol: {:x?} in B0, {now:x?} in B1",
            p.b0_syms
        ));
    }
    let s = secs
        .iter()
        .find(|s| s.name == SECTION)
        .ok_or("B1 has no perry_snap_image section")?;
    if s.addr != IMAGE_ADDR || s.size > IMAGE_WINDOW {
        return Err(format!(
            "the section is at {:#x}+{:#x}, wanted {IMAGE_ADDR:#x} within {IMAGE_WINDOW:#x}",
            s.addr, s.size
        ));
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
        "proof sections-equal={n} program-headers-equal={np} boundary-symbols-equal=yes section-at={:#x}\ndecoded-equals-image=yes\nwords total={} dropped-pristine={} kept={}\ndropped-by-static: {}\n-- B0 dump --\n{}-- section dump --\n{}",
        s.addr,
        p.total_words,
        p.dropped,
        got.words.len(),
        p.dropped_by,
        p.b0_dump,
        format_read::dump(&got)
    ))
}

/// A runtime static (a Rust path) of more than one word: a container whose
/// scalar words only mean something with its pointers.
fn runtime_container(path: &str, size: u64) -> bool {
    path.contains("::") && size > 8
}

/// A per-site concat table is a cache: slot k is 0 or the string the miss arm
/// builds, so a changed word must be a string value (short string or string
/// cell) and is then pristine; anything else stops.
const STRING_TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const SHORT_STRING_BITS: u64 = 0x7FF9_0000_0000_0000;
const STRING_BITS: u64 = 0x7FFF_0000_0000_0000;

fn concat_site_word(v: &Val) -> Judge {
    let string = match v {
        Val::Scalar(x) => x & STRING_TAG_MASK == SHORT_STRING_BITS,
        Val::Cell { hi, bare, .. } => !*bare && u64::from(*hi) << 48 == STRING_BITS,
        _ => false,
    };
    if string {
        Judge::Pristine
    } else {
        Judge::Stop("a concat-site slot holds a value that is not a string")
    }
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
        assert_eq!(
            sym_at(&t.mem, 0x600014).map(|s| s.name.as_str()),
            Some("perry_global_x")
        );
        assert!(sym_at(&t.mem, 0x600008).is_none());
        let s = parse_secs("  [ 0]                   NULL            0000000000000000 000000 000000 00      0   0  0\n  [16] .tdata            PROGBITS        0000000000600000 001000 000010 00 WAT  0   0  8\n");
        assert_eq!(
            s,
            vec![Sec {
                name: ".tdata".into(),
                addr: 0x600000,
                size: 0x10
            }]
        );
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
        Cell {
            obj_type: 2,
            flags: 0x15,
            reserved: 0,
            size: 24,
            bytes: vec![7; 24],
            slots,
        }
    }

    const RES: &str =
        "_RNvNtNtNtCsXbwMRDrjNy_13perry_runtime5arena6region11reservation11RESERVATION.0.llvm.1171";

    #[test]
    fn pristine_names_match_exact_paths_not_substrings() {
        let o = Val::Opaque(0x7f00_0000_1000);
        assert_eq!(judge(RES, &o), Judge::Pristine);
        assert_eq!(judge("x_reservation_y", &o), Judge::Keep);
        let longer =
            "_RNvNtNtNtCsXbwMRDrjNy_13perry_runtime5arena6region11reservation13RESERVATION_X";
        assert_eq!(
            judge(longer, &o),
            Judge::Keep,
            "a longer last component is another static"
        );
        assert_eq!(judge("heap_main", &o), Judge::Pristine);
        assert_eq!(judge("not_heap_main_x", &o), Judge::Keep);
    }

    #[test]
    fn the_unsound_pristine_entries_stop_the_image() {
        let pi = "_RNvNtNtCsXbwMRDrjNy_13perry_runtime6object11class_image13PRIMARY_IMAGE";
        let cl = "_RNvNtCsXbwMRDrjNy_13perry_runtime7tls_hot10CLAIM_LOCK";
        for v in [Val::Scalar(0x77), Val::Opaque(0x7f00_0000_1000)] {
            assert!(matches!(judge(pi, &v), Judge::Stop(_)), "class tables");
            assert!(matches!(judge(cl, &v), Judge::Stop(_)), "claim lock");
        }
    }

    #[test]
    fn a_pending_timer_stops_and_an_empty_store_does_not() {
        let t = "_RNvNtNtCsXbwMRDrjNy_13perry_runtime5timer5store20PRIMARY_REFED_TIMERS";
        assert!(matches!(judge(t, &Val::Scalar(1)), Judge::Stop(_)));
        assert_eq!(judge(t, &Val::Scalar(0)), Judge::Keep);
    }

    #[test]
    fn the_thread_token_is_pristine_only_when_the_primary_alone_claimed() {
        let n = "_RNvNtNtNtCsXbwMRDrjNy_13perry_runtime6object13field_get_set21class_object_template17NEXT_THREAD_TOKEN";
        assert_eq!(judge(n, &Val::Scalar(2)), Judge::Pristine);
        assert!(matches!(judge(n, &Val::Scalar(3)), Judge::Stop(_)));
    }

    #[test]
    fn a_symbol_space_word_outside_a_pristine_static_is_unresolved() {
        let t = parse_syms("    3: 0000000000600038     8 OBJECT  LOCAL  DEFAULT   24 OTHER\n");
        let ws = [word(format::WORD_DATA, 0x38, Val::Sym(0x7c07_79e0_0010))];
        let c = classify(&ws, &t, 0x600000);
        assert!(c.kept.is_empty());
        assert!(
            c.unresolved[0].contains("symbol-space"),
            "{:?}",
            c.unresolved
        );
    }

    #[test]
    fn program_headers_must_be_equal_apart_from_the_image_segment() {
        let t0 = "Program Headers:\n  Type Offset VirtAddr PhysAddr FileSiz MemSiz Flg Align\n  LOAD 0x000000 0x0000000000400000 0x0000000000400000 0x001000 0x001000 R E 0x1000\n  LOAD 0x001000 0x0000000040000000 0x0000000040000000 0x000008 0x000008 RW  0x1000\n\nSection to Segment mapping:\n";
        let t1 = t0.replace("0x000008 0x000008", "0x000a00 0x000a00");
        let (a, b) = (parse_phdrs(t0), parse_phdrs(&t1));
        assert_eq!(phdr_proof(&a, &b), Ok(1), "only the image segment differs");
        let t2 = t0.replace("0x001000 0x001000 R E", "0x002000 0x002000 R E");
        assert!(
            phdr_proof(&a, &parse_phdrs(&t2)).is_err(),
            "a moved segment must fail"
        );
    }

    #[test]
    fn a_changed_word_of_a_pristine_static_is_judged_by_the_pristine_list() {
        let t = parse_syms("Symbol table '.symtab' contains 2 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n    10: 0000000000600000     8 OBJECT  LOCAL  DEFAULT   24 theap_main\n    11: 0000000000600008     8 OBJECT  LOCAL  DEFAULT   24 SOME_CACHE\n");
        let ws = [
            word(format::WORD_DATA, 0, Val::Opaque(0x7f00_0000_1000)),
            word(format::WORD_DATA, 8, Val::Opaque(0x7f00_0000_2000)),
        ];
        let r = classify(&ws, &t, 0x600000);
        assert_eq!(
            r.dropped, 1,
            "the allocator static is pristine whatever it holds"
        );
        assert_eq!(r.unresolved.len(), 1, "{:?}", r.unresolved);
        assert!(r.unresolved[0].contains("SOME_CACHE"));
    }

    #[test]
    fn a_stale_unresolved_list_is_removed() {
        let d = std::env::temp_dir().join(format!("img_stale_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let b0 = d.join("b0");
        std::fs::write(unresolved_path(&b0), "old").unwrap();
        clear_unresolved(&b0);
        assert!(!unresolved_path(&b0).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cells_become_link_time_constants() {
        let tag = Val::Cell {
            hi: 0x7FFD,
            bare: false,
            index: 1,
        };
        let img = Image {
            cells: vec![
                cell(vec![(8, tag)]),
                cell(vec![(
                    16,
                    Val::Ext {
                        hi: 0,
                        bare: true,
                        region: format::REGION_EXE,
                        off: 0x40,
                    },
                )]),
            ],
            ..Image::default()
        };
        let ll = image_ll(&img, &[], 0).expect("ll");
        assert!(
            ll.contains(&format!(
                "@perry_snap_cell_1, i64 {})",
                ((0x7FFDu64 << 48) + 8) as i64
            )),
            "{ll}"
        );
        assert!(ll.contains("ptr @__executable_start, i64 64"), "{ll}");
        assert!(ll.contains("section \"perry_snap_image\""));
        let bad = Image {
            cells: vec![cell(vec![(8, Val::Opaque(0x7f00_0000_2000))])],
            ..Image::default()
        };
        assert!(image_ll(&bad, &[], 0).is_err());
        let idx = Image {
            cells: vec![cell(vec![(
                8,
                Val::Cell {
                    hi: 0,
                    bare: true,
                    index: 5,
                },
            )])],
            ..Image::default()
        };
        assert!(image_ll(&idx, &[], 0).is_err());
    }

    #[test]
    fn the_section_decodes_to_its_image() {
        let v = 0x1000_0000u64;
        let mut s = vec![0u8; 300];
        let mut put = |o: usize, x: u64| s[o..o + 8].copy_from_slice(&x.to_le_bytes());
        for (o, x) in [
            (0, SECTION_MAGIC),
            (8, 1),
            (16, 2),
            (24, 1),
            (32, 7),
            (40, v + 88),
            (48, v + 104),
            (56, v + 152),
            (64, v + 208),
            (72, 3),
            (80, v + 224),
        ] {
            put(o, x);
        }
        put(88, v + 184);
        put(96, 2 | 0x15 << 8 | 24 << 32);
        for (o, x) in [
            (104, 0),
            (112, 8),
            (120, 1),
            (128, 0),
            (136, 16),
            (144, 5 | 3 << 8),
        ] {
            put(o, x);
        }
        for (o, x) in [(152, 0), (160, 0x600010), (168, 0), (176, 5)] {
            put(o, x);
        }
        put(192, (0x7FFDu64 << 48) | (v + 184 + 8));
        let blob = format::claims_blob(&[b"ab".to_vec(), b"c".to_vec()]);
        s[208..208 + blob.len()].copy_from_slice(&blob);
        let rec = format::RegRec {
            key: b"k".to_vec(),
            init_off: 9,
            status: 2,
            exports: Some(Val::Cell {
                hi: 0x7FFD,
                bare: false,
                index: 0,
            }),
            factory: None,
        };
        let rb = format::registry_blob(std::slice::from_ref(&rec));
        s[224..224 + rb.len()].copy_from_slice(&rb);
        let b = Bases {
            strings: 0,
            layouts: 0,
            atoms: 0,
            exe: 0x400000,
            data_start: 0x600000,
        };
        let got = decode_section(&s, v, &b).expect("decode");
        let want = Image {
            cells: vec![Cell {
                obj_type: 2,
                flags: 0x15,
                reserved: 0,
                size: 24,
                bytes: vec![0u8; 24],
                slots: vec![
                    (
                        8,
                        Val::Cell {
                            hi: 0x7FFD,
                            bare: false,
                            index: 0,
                        },
                    ),
                    (16, Val::Derived(3)),
                ],
            }],
            words: vec![word(format::WORD_DATA, 0x10, Val::Scalar(5))],
            stops: vec![],
            pending: 7,
            claims_from: 3,
            claims: vec![b"ab".to_vec(), b"c".to_vec()],
            registry: vec![rec],
            mint: Vec::new(),
            canon: None,
        };
        assert_eq!(got, want);
    }

    #[test]
    fn registry_statics_are_dropped_only_when_a_record_covers_them() {
        let t = parse_syms("Symbol table '.symtab' contains 1 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n    10: 0000000000600000     8 OBJECT  LOCAL  DEFAULT   24 MODULE_PATH_REGISTRY\n");
        let ws = [word(format::WORD_DATA, 0, Val::Opaque(0x7f00_0000_1000))];
        assert_eq!(classify(&ws, &t, 0x600000).unresolved.len(), 1);
        let r = classify_with(&ws, &t, 0x600000, true);
        assert_eq!((r.dropped, r.unresolved.len()), (1, 0));
    }

    #[test]
    fn a_derived_slot_is_a_zero_word_in_the_ir() {
        let img = Image {
            cells: vec![Cell {
                obj_type: 8,
                flags: 0,
                reserved: 0,
                size: 16,
                bytes: vec![0u8; 16],
                slots: vec![(8, Val::Derived(1))],
            }],
            ..Image::default()
        };
        let ll = image_ll(&img, &[], 0x600000).expect("ir");
        assert!(ll.contains("[3 x i64] [i64 0, i64 8, i64 261]"), "{ll}");
        assert!(ll.contains("@perry_snap_registry"));
    }

    #[test]
    fn a_moved_section_fails_the_proof() {
        let a = vec![
            Sec {
                name: ".data".into(),
                addr: 0x600000,
                size: 16,
            },
            Sec {
                name: ".bss".into(),
                addr: 0x600010,
                size: 8,
            },
        ];
        assert_eq!(proof(&a, &a), Ok(2));
        let mut b = a.clone();
        b[1].addr += 0x1000;
        assert!(proof(&a, &b).is_err());
        let mut c = a.clone();
        c[0].size += 8;
        assert!(proof(&a, &c).is_err());
        assert!(proof(&a, &a[..1]).is_err());
    }

    #[test]
    fn every_other_section_must_be_equal_and_all_moves_are_listed() {
        let sec = |n: &str, a: u64, z: u64| Sec {
            name: n.into(),
            addr: a,
            size: z,
        };
        // The image section itself differs in size and does not count.
        let b0 = vec![
            sec(".text", 0x401000, 0x100),
            sec(".data", 0x600000, 16),
            sec(SECTION, IMAGE_ADDR, 8),
        ];
        let b1 = vec![
            sec(".text", 0x401000, 0x100),
            sec(".data", 0x600000, 16),
            sec(SECTION, IMAGE_ADDR, 0x900),
        ];
        assert_eq!(proof(&b0, &b1), Ok(2));
        // Two sections moved and one appeared: all three are named.
        let mut c = b1.clone();
        c[0].addr += 0x38;
        c[1].size += 8;
        c.push(sec(".extra", 0x700000, 4));
        let e = proof(&b0, &c).unwrap_err();
        assert!(
            e.contains(".text") && e.contains(".data") && e.contains(".extra"),
            "{e}"
        );
        assert!(e.starts_with("3 section(s) differ"), "{e}");
    }

    #[test]
    fn the_image_address_is_checked_against_the_bss() {
        let sec = |a: u64| {
            vec![Sec {
                name: SECTION.into(),
                addr: a,
                size: 8,
            }]
        };
        assert!(check_b0_layout(&sec(IMAGE_ADDR), 0x80_0000).is_ok());
        assert!(check_b0_layout(&sec(IMAGE_ADDR), IMAGE_ADDR - IMAGE_HEADROOM + 1).is_err());
        assert!(check_b0_layout(&sec(IMAGE_ADDR + 0x1000), 0x80_0000).is_err());
        assert!(check_b0_layout(&[], 0x80_0000).is_err());
        assert!(placeholder_ll().contains(SECTION));
    }
}

/// The reasons an image stops, one named line per (reason, cell type).
fn stop_summary(stops: &[format::Stop]) -> String {
    let mut by: std::collections::BTreeMap<(u8, u8), (usize, u64, u32, u32)> = Default::default();
    for t in stops {
        let e = by
            .entry((t.kind, t.obj_type))
            .or_insert((0, t.value, t.off, t.cell));
        e.0 += 1;
    }
    let ty = |t: u8| match t {
        5 => "promise",
        8 => "Map",
        20 => "regular expression",
        21 => "regular expression program",
        35 => "Map store",
        41 => "regex cache",
        _ => "cell",
    };
    let mut parts = Vec::new();
    for ((kind, t), (n, value, off, cell)) in by {
        parts.push(match kind {
            format::STOP_NO_FORM => format!("{n} {} cell(s) (type {t}) have no image form", ty(t)),
            format::STOP_SLOT_OUTSIDE => format!("{n} slot(s) outside their cell (type {t})"),
            format::STOP_NOT_CELL_START => format!("{n} slot(s) of type {t} name an address that is not a cell start"),
            format::STOP_CLAIM_KEY => format!("{n} hot-slot claim key(s) too long or not UTF-8"),
            format::STOP_SHAPES_MINTED => {
                let band = |k: u32| (value >> (16 * k)) & 0xffff;
                format!(
                    "{} shape record(s) were minted between the baseline and the cut (static {}, ordinary {}, dictionary {}, exotic {}), {} record(s) present at the baseline changed, {} are gone, and the image has no shape slab",
                    band(0) + band(1) + band(2) + band(3), band(0), band(1), band(2), band(3), off, cell
                )
            }
            format::STOP_MINT_RECORD => {
                format!(
                    "{} shape record(s) were minted between the baseline and the cut (static {}, ordinary {}, dictionary {}, exotic {}); the mint record was written and verified ({} record(s), {} byte(s)); restore does not replay it yet",
                    value & 0xffff_ffff, cell & 0xffff, off & 0xffff, cell >> 16, off >> 16, value & 0xffff_ffff, value >> 32
                )
            }
            format::STOP_MINT_CENSUS => {
                if t == 0 {
                    format::mint_census_text(stops).unwrap_or_default()
                } else {
                    String::new()
                }
            }
            format::STOP_MINT_KEYS => format!("mint record {cell} (shape id {value}): its key list is neither an image cell nor binary data ({off} record(s) in all)"),
            format::STOP_MINT_CANONICAL => format!("mint record {cell} (shape id {value}) keys a canonical list; replay needs the canonical-keys record ({off} record(s) in all)"),
            format::STOP_MINT_PROTO_WORD => format!("mint record {cell} (shape id {value}): its prototype word is not an image cell ({off} record(s) in all)"),
            format::STOP_MINT_NO_DESC => format!("mint record {cell} (shape id {value}) is a runtime layout's first birth whose descriptor cell was not found ({off} record(s) in all)"),
            format::STOP_MINT_ROW_GAP => format!("mint record {cell} (shape id {value}) leaves a row gap; replay cannot position rows ({off} record(s) in all)"),
            format::STOP_CANON_FREE => format!("the canonical trie freed {value} node(s) between the baseline and the cut (a full trace pruned lists; {off} node(s) present at the baseline were touched, the first {cell}); the record cannot carry the free list"),
            format::STOP_CANON_ADDR => format!("trie node {cell}: its backing is not an image cell (address {value:#x}; {off} node(s) in all)"),
            format::STOP_CANON_ORDER => format!("trie node {cell} was allocated out of id order ({off} node(s) in all)"),
            format::STOP_CANON_RECORD => format!(
                "the canonical-keys record was written ({off} byte(s)) and {}; trie: new {}, changed {}",
                if cell == 0 { "reads back equal" } else { "does not read back equal" },
                value & 0xffff_ffff,
                value >> 32
            ),
            format::STOP_MINT_SUCCESSOR => format!("mint record {cell} (shape id {value}) was minted by a slot-list successor (holes, tombstone or delete); replay has no route ({off} record(s) in all)"),
            format::STOP_MINT_MISMATCH if off == format::MINT_BAD_LAZY_ROW => {
                format!("mint record {cell} (shape id {value}) is on a static row that is neither a runtime layout nor a lazy image row naming its word")
            }
            format::STOP_MINT_MISMATCH => format!(
                "mint record {cell} (shape id {value}) does not rebuild to the live record: {}",
                match off {
                    format::MINT_BAD_DICTIONARY => "a dictionary-band record has no form",
                    format::MINT_BAD_IDENTITY => "its facts do not reach the recorded word",
                    format::MINT_BAD_FIELDS => "a recorded field differs from the live record",
                    _ => "a row or an id is out of mint order",
                }
            ),
            format::STOP_SHAPE_COUNTERS => format!("the shape counters moved between the baseline and the cut ({} id(s) drawn, {} semantic generation(s))", value & 0xffff_ffff, value >> 32),
            format::STOP_TOKENS => format!("more than the primary's class-template token was claimed (next token {value})"),
            format::STOP_RAW_HEAP_WORD => format!("{n} {} cell(s) (type {t}) hold a raw address outside the binary in an untraced word (first at offset {off}, value {value:#x})", ty(t)),
            format::STOP_MAP_KEYS => format!("{n} Map store(s) are keyed by something other than strings and numbers"),
            format::STOP_REGISTRY => format!("{n} module-path registry entr(ies) have no replay form"),
            k => format!("{n} stop(s) of kind {k}"),
        });
    }
    parts.retain(|p| !p.is_empty());
    parts.join("; ")
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn the_class_table_stop_names_each_growing_table() {
        let st = |cell: u32, value: u64| format::Stop {
            cell,
            obj_type: 0,
            kind: format::STOP_CLASS_TABLES,
            off: 0,
            value,
        };
        assert!(class_table_message(&[]).is_none());
        let m = class_table_message(&[st(0, 5), st(18, 7)]).unwrap();
        assert!(
            m.contains("12 row(s)") && m.contains("declarations +5") && m.contains("names +7"),
            "{m}"
        );
    }

    const SY: &str = "Symbol table '.symtab' contains 5 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n    10: 0000000000600000    48 OBJECT  LOCAL  DEFAULT   24 _RNvNtNtCsXbwMRDrjNy_13perry_runtime6string6intern5ATOMS\n    11: 0000000000600030     8 OBJECT  LOCAL  DEFAULT   24 _RNvNtNtCsXbwMRDrjNy_13perry_runtime6string6intern4SLOT\n    12: 0000000000600040   256 OBJECT  LOCAL  DEFAULT   24 perry_concat_site_dep__1\n    13: 0000000000600200    16 OBJECT  LOCAL  DEFAULT   24 _RNvNtNtCsXbwMRDrjNy_13perry_runtime6string6intern5STATE\n    14: 0000000000000040     8 TLS     LOCAL  DEFAULT   20 perry_tls_y\n";

    fn w(region: u8, off: u64, val: Val) -> Word {
        Word { region, off, val }
    }

    #[test]
    fn a_scalar_word_of_a_stopped_static_is_not_kept() {
        let t = parse_syms(SY);
        let ws = [
            w(format::WORD_DATA, 0, Val::Opaque(0x7f00_0000_1000)),
            w(format::WORD_DATA, 0x18, Val::Scalar(0x2be)),
            w(format::WORD_DATA, 0x30, Val::Scalar(0x3e)),
        ];
        let c = classify(&ws, &t, 0x600000);
        assert_eq!(
            c.kept.len(),
            1,
            "only the single-word static stays: {:?}",
            c.unresolved
        );
        assert!(c
            .unresolved
            .iter()
            .any(|u| u.contains("ATOMS") && u.contains("stops as one unit")));
    }

    #[test]
    fn the_scalar_words_of_a_runtime_container_are_not_kept() {
        let t = parse_syms(SY);
        let ws = [
            w(format::WORD_DATA, 0x208, Val::Scalar(5)),
            w(format::WORD_DATA, 0x30, Val::Scalar(7)),
        ];
        let c = classify(&ws, &t, 0x600000);
        assert_eq!(c.kept.len(), 1, "{:?}", c.unresolved);
        assert!(c.unresolved.len() == 1 && c.unresolved[0].contains("STATE"));
    }

    #[test]
    fn a_concat_site_slot_is_pristine_only_while_it_holds_a_string() {
        let t = parse_syms(SY);
        let s = Val::Scalar(0x7FF9_0000_0000_6e30);
        let ok = classify(
            &[
                w(format::WORD_DATA, 0x40, s.clone()),
                w(format::WORD_DATA, 0x48, s),
            ],
            &t,
            0x600000,
        );
        assert!(
            ok.unresolved.is_empty() && ok.dropped == 2,
            "{:?}",
            ok.unresolved
        );
        let bad = classify(
            &[w(format::WORD_DATA, 0x40, Val::Scalar(0x2a))],
            &t,
            0x600000,
        );
        assert!(
            bad.unresolved.iter().any(|u| u.contains("not a string")),
            "{:?}",
            bad.unresolved
        );
    }

    #[test]
    fn a_tls_word_outside_every_named_symbol_is_unnamed() {
        let t = parse_syms(SY);
        let c = classify(
            &[w(format::WORD_TLS, 0x2ef88, Val::Scalar(2))],
            &t,
            0x600000,
        );
        assert!(
            c.unresolved.iter().any(|u| u.contains("unnamed TLS word")),
            "{:?}",
            c.unresolved
        );
    }

    #[test]
    fn a_record_covers_only_the_registry_statics() {
        let t = parse_syms(SY);
        let ws = [w(format::WORD_DATA, 0, Val::Opaque(0x7f00_0000_1000))];
        let r = classify_with(&ws, &t, 0x600000, true);
        assert_eq!(
            (r.dropped, r.unresolved.len()),
            (0, 1),
            "{:?}",
            r.unresolved
        );
    }
}

/// The class-table stop text: the total rows and which table grew by how many.
fn class_table_message(stops: &[format::Stop]) -> Option<String> {
    let rows: Vec<_> = stops
        .iter()
        .filter(|t| t.kind == format::STOP_CLASS_TABLES)
        .collect();
    if rows.is_empty() {
        return None;
    }
    let total: u64 = rows.iter().map(|t| t.value).sum();
    let per: Vec<String> = rows
        .iter()
        .map(|t| {
            format!(
                "{} +{}",
                format::CLASS_TABLE_NAMES
                    .get(t.cell as usize)
                    .copied()
                    .unwrap_or("?"),
                t.value
            )
        })
        .collect();
    Some(format!(
        "the class tables grew by {total} row(s) between the baseline and the cut ({}), and the image has no form that restores them",
        per.join(", ")
    ))
}
