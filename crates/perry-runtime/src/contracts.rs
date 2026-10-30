//! wpfacts contract build (feature `contract-checks`, design 6.2): the
//! runtime checks the compiler's whole-program claims at every dynamic
//! operation that could contradict them, and aborts naming the fact.
//!
//! The claims live in each module's read-only `.perry_facts` section
//! (emitted only when compiling with `PERRY_CHECK_CONTRACTS=1`). They are
//! keyed by class id; object-literal templates without a class id are
//! covered by the static checkers, not here.
//!
//! Checked:
//! - HARD operations (delete, defineProperty/ies, setPrototypeOf, freeze/
//!   seal/preventExtensions, `new Proxy` target) on an instance of a class
//!   claimed prefix-stable (F3), and on a claimed class's prototype (or an
//!   ancestor's);
//! - `Object.create(C.prototype)` for a claimed prefix-stable C (E10);
//! - every generic (runtime slow-path) named store: the value's kind must be
//!   inside the claimed kind (F2), and a fixed-layout class (F3) may gain no
//!   key outside its birth keys;
//! - runtime writers that never run in construction (`Object.assign`,
//!   `Reflect.set`, `Error.captureStackTrace`): no write to a key claimed
//!   readonly (F1).
//!
//! Release builds compile none of this.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;
const TAG_MARKER: u64 = 0x7FFC_0000_0000_0000;

const K_NUM: u16 = 1;
const K_STR: u16 = 2;
const K_BOOL: u16 = 4;
const K_NULL: u16 = 8;
const K_UNDEF: u16 = 16;
const K_BIGINT: u16 = 32;
const K_SYMBOL: u16 = 64;
const K_OBJ: u16 = 128;

struct KeyClaim {
    key: Vec<u8>,
    readonly: bool,
    birth: bool,
    kind: u16,
}

struct Claim {
    name: String,
    prefix_stable: bool,
    fixed_layout: bool,
    ancestors: Vec<u32>,
    keys: Vec<KeyClaim>,
}

impl Claim {
    /// Several records describe one class id (structurally identical literal
    /// classes of several modules share it; per-unit facts describe a class
    /// once per unit that declares it, each for its own instances): every
    /// instance is checked against what all of them claim, the weakest.
    fn meet(&mut self, o: Claim) {
        self.prefix_stable &= o.prefix_stable;
        let same_keys = self.keys.len() == o.keys.len()
            && self
                .keys
                .iter()
                .all(|k| o.keys.iter().any(|ok| ok.key == k.key));
        self.fixed_layout &= o.fixed_layout && same_keys;
        for a in o.ancestors {
            if !self.ancestors.contains(&a) {
                self.ancestors.push(a);
            }
        }
        for k in self.keys.iter_mut() {
            match o.keys.iter().find(|ok| ok.key == k.key) {
                Some(ok) => {
                    k.readonly &= ok.readonly;
                    k.birth &= ok.birth;
                    k.kind |= ok.kind;
                }
                None => {
                    k.readonly = false;
                    k.birth = false;
                    k.kind = 0xFFFF;
                }
            }
        }
        for ok in o.keys {
            if !self.keys.iter().any(|k| k.key == ok.key) {
                self.keys.push(KeyClaim {
                    key: ok.key,
                    readonly: false,
                    birth: false,
                    kind: 0xFFFF,
                });
            }
        }
    }
}

/// Every contract build references this symbol from its `.perry_facts`
/// blob (`perry-codegen` `emit_contract_section`), so a contract build
/// linked against a runtime without `contract-checks` is a link error that
/// names the feature, not a binary that silently checks nothing.
#[no_mangle]
#[used]
pub static perry_contract_build_needs_runtime_feature_contract_checks: u8 = 1;

static CLAIMS: OnceLock<HashMap<u32, Claim>> = OnceLock::new();
static CHECKS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Inside a runtime writer that never runs in construction
    /// (`Object.assign`): generic stores also check readonly claims.
    static RUNTIME_WRITER: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

fn claims() -> &'static HashMap<u32, Claim> {
    CLAIMS.get_or_init(|| {
        let mut out = HashMap::new();
        for sec in facts_sections() {
            parse_into(sec, &mut out);
        }
        if std::env::var_os("PERRY_CONTRACTS_REPORT").is_some() {
            eprintln!(
                "perry: wpfacts contracts: {} class claims loaded",
                out.len()
            );
            unsafe {
                libc::atexit(report_at_exit);
            }
        }
        out
    })
}

extern "C" fn report_at_exit() {
    eprintln!(
        "perry: wpfacts contracts: {} checks against {} class claims, 0 violations",
        CHECKS.load(Ordering::Relaxed),
        CLAIMS.get().map(|c| c.len()).unwrap_or(0)
    );
}

fn rd_u16(b: &[u8], at: &mut usize) -> Option<u16> {
    let v = u16::from_le_bytes(b.get(*at..*at + 2)?.try_into().ok()?);
    *at += 2;
    Some(v)
}

fn rd_u32(b: &[u8], at: &mut usize) -> Option<u32> {
    let v = u32::from_le_bytes(b.get(*at..*at + 4)?.try_into().ok()?);
    *at += 4;
    Some(v)
}

fn rd_bytes<'a>(b: &'a [u8], at: &mut usize, n: usize) -> Option<&'a [u8]> {
    let v = b.get(*at..*at + n)?;
    *at += n;
    Some(v)
}

/// Blobs are concatenated by the linker, each 8-byte aligned.
fn parse_into(sec: &[u8], out: &mut HashMap<u32, Claim>) {
    let mut at = 0usize;
    while at + 12 <= sec.len() {
        if &sec[at..at + 4] != b"PFCT" {
            at += 8;
            continue;
        }
        let mut p = at + 4;
        let Some(end) = parse_blob(sec, &mut p, out) else {
            // A malformed blob is a compiler bug: fail loudly.
            eprintln!("perry: wpfacts contracts: malformed .perry_facts blob at {at}");
            std::process::abort();
        };
        at = end.div_ceil(8) * 8;
    }
}

fn parse_blob(b: &[u8], at: &mut usize, out: &mut HashMap<u32, Claim>) -> Option<usize> {
    let version = rd_u32(b, at)?;
    if version != 2 {
        return None;
    }
    let count = rd_u32(b, at)?;
    for _ in 0..count {
        let cid = rd_u32(b, at)?;
        let flags = rd_u32(b, at)?;
        let nanc = rd_u16(b, at)? as usize;
        let mut ancestors = Vec::with_capacity(nanc);
        for _ in 0..nanc {
            ancestors.push(rd_u32(b, at)?);
        }
        let nlen = rd_u16(b, at)? as usize;
        let name = String::from_utf8_lossy(rd_bytes(b, at, nlen)?).to_string();
        let nkeys = rd_u16(b, at)? as usize;
        let mut keys = Vec::with_capacity(nkeys);
        for _ in 0..nkeys {
            let klen = rd_u16(b, at)? as usize;
            let key = rd_bytes(b, at, klen)?.to_vec();
            let fl = *b.get(*at)?;
            *at += 1;
            let kind = rd_u16(b, at)?;
            keys.push(KeyClaim {
                key,
                readonly: fl & 1 != 0,
                birth: fl & 4 != 0,
                kind,
            });
        }
        let claim = Claim {
            name,
            prefix_stable: flags & 1 != 0,
            fixed_layout: flags & 2 != 0,
            ancestors,
            keys,
        };
        match out.entry(cid) {
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(claim);
            }
            std::collections::hash_map::Entry::Occupied(mut o) => o.get_mut().meet(claim),
        }
    }
    Some(*at)
}

/// Every loaded `.perry_facts` section of the executable.
fn facts_sections() -> Vec<&'static [u8]> {
    #[cfg(target_os = "linux")]
    {
        linux_section(b".perry_facts").into_iter().collect()
    }
    #[cfg(target_vendor = "apple")]
    {
        apple_section().into_iter().collect()
    }
    #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
    {
        Vec::new()
    }
}

#[cfg(target_vendor = "apple")]
fn apple_section() -> Option<&'static [u8]> {
    unsafe extern "C" {
        fn _dyld_get_image_header(idx: u32) -> *const libc::c_void;
        fn getsectiondata(
            mhp: *const libc::c_void,
            segname: *const std::os::raw::c_char,
            sectname: *const std::os::raw::c_char,
            size: *mut std::os::raw::c_ulong,
        ) -> *mut u8;
    }
    unsafe {
        let mh = _dyld_get_image_header(0);
        if mh.is_null() {
            return None;
        }
        let mut size: std::os::raw::c_ulong = 0;
        let p = getsectiondata(mh, c"__DATA".as_ptr(), c"__perry_facts".as_ptr(), &mut size);
        if p.is_null() || size == 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(p, size as usize))
    }
}

/// The main executable's section `name`: section headers from
/// `/proc/self/exe`, load bias from the first `dl_iterate_phdr` image.
#[cfg(target_os = "linux")]
fn linux_section(name: &[u8]) -> Option<&'static [u8]> {
    use std::os::unix::fs::FileExt;
    let f = std::fs::File::open("/proc/self/exe").ok()?;
    let mut eh = [0u8; 64];
    f.read_exact_at(&mut eh, 0).ok()?;
    if &eh[0..4] != b"\x7fELF" || eh[4] != 2 {
        return None;
    }
    let shoff = u64::from_le_bytes(eh[0x28..0x30].try_into().ok()?);
    let shentsize = u16::from_le_bytes(eh[0x3a..0x3c].try_into().ok()?) as u64;
    let shnum = u16::from_le_bytes(eh[0x3c..0x3e].try_into().ok()?) as u64;
    let shstrndx = u16::from_le_bytes(eh[0x3e..0x40].try_into().ok()?) as u64;
    if shentsize < 64 || shnum == 0 || shnum > 65535 {
        return None;
    }
    let mut table = vec![0u8; (shentsize * shnum) as usize];
    f.read_exact_at(&mut table, shoff).ok()?;
    let sh = |i: u64| &table[(i * shentsize) as usize..(i * shentsize + 64) as usize];
    let strtab = sh(shstrndx);
    let str_off = u64::from_le_bytes(strtab[0x18..0x20].try_into().ok()?);
    let str_size = u64::from_le_bytes(strtab[0x20..0x28].try_into().ok()?);
    if str_size > 64 << 20 {
        return None;
    }
    let mut names = vec![0u8; str_size as usize];
    f.read_exact_at(&mut names, str_off).ok()?;
    for i in 0..shnum {
        let s = sh(i);
        let nm = u32::from_le_bytes(s[0..4].try_into().ok()?) as usize;
        let end = names[nm.min(names.len())..]
            .iter()
            .position(|&c| c == 0)
            .map(|p| nm + p)?;
        if &names[nm..end] != name {
            continue;
        }
        let addr = u64::from_le_bytes(s[0x10..0x18].try_into().ok()?) as usize;
        let size = u64::from_le_bytes(s[0x20..0x28].try_into().ok()?) as usize;
        let bias = main_image_bias()?;
        return Some(unsafe { std::slice::from_raw_parts((bias + addr) as *const u8, size) });
    }
    None
}

#[cfg(target_os = "linux")]
fn main_image_bias() -> Option<usize> {
    #[repr(C)]
    struct Info {
        addr: usize,
        name: *const std::os::raw::c_char,
    }
    unsafe extern "C" {
        fn dl_iterate_phdr(
            cb: unsafe extern "C" fn(*mut Info, usize, *mut std::ffi::c_void) -> i32,
            data: *mut std::ffi::c_void,
        ) -> i32;
    }
    unsafe extern "C" fn first(info: *mut Info, _: usize, data: *mut std::ffi::c_void) -> i32 {
        *data.cast::<Option<usize>>() = Some((*info).addr);
        1
    }
    let mut out: Option<usize> = None;
    unsafe {
        dl_iterate_phdr(first, (&mut out as *mut Option<usize>).cast());
    }
    out
}

/// The kind bit of a NaN-boxed value (compiler `KindFact::mask` layout).
fn kind_of(v: f64) -> u16 {
    let bits = v.to_bits();
    match bits & TAG_MASK {
        POINTER_TAG => {
            if unsafe { crate::symbol::js_is_symbol(v) } != 0 {
                K_SYMBOL
            } else {
                K_OBJ
            }
        }
        JS_HANDLE_TAG => K_OBJ,
        STRING_TAG | SHORT_STRING_TAG => K_STR,
        INT32_TAG => K_NUM,
        BIGINT_TAG => K_BIGINT,
        TAG_MARKER => match bits & 0xFF {
            2 => K_NULL,
            3 | 4 => K_BOOL,
            _ => K_UNDEF,
        },
        _ => K_NUM,
    }
}

/// The class id of an ordinary object behind `addr`, proven by the
/// allocator (never an unchecked header read).
fn class_id_at(addr: usize) -> Option<u32> {
    if crate::value::addr_class::is_handle_band(addr) {
        return None;
    }
    unsafe {
        crate::value::addr_class::try_read_tracked_gc_header_of_type(
            addr,
            crate::gc::GC_TYPE_OBJECT,
        )?;
        Some((*(addr as *const crate::object::ObjectHeader)).class_id)
    }
}

fn class_id_of(v: f64) -> Option<u32> {
    let bits = v.to_bits();
    if bits & TAG_MASK != POINTER_TAG {
        return None;
    }
    class_id_at((bits & POINTER_MASK) as usize)
}

#[cold]
fn violate(fact: &str, c: &Claim, cid: u32, op: &str, detail: &str) -> ! {
    eprintln!(
        "perry: wpfacts contract violated: fact {fact} {} (class_id {cid}) contradicted by {op}{detail}",
        c.name
    );
    std::process::abort();
}

/// The class whose prototype `v` is: a `C.prototype` reference immediate or
/// the declared prototype object itself.
fn proto_class_id(v: f64) -> Option<u32> {
    if let Some(cid) = crate::object::class_prototype_ref_id(v) {
        return Some(cid);
    }
    let bits = v.to_bits();
    if bits & TAG_MASK != POINTER_TAG {
        return None;
    }
    crate::object::class_id_for_decl_prototype_object((bits & POINTER_MASK) as usize)
}

/// A HARD operation on the object `v` (E5–E9).
pub(crate) fn hard_value(v: f64, op: &str) {
    if let Some(cid) = proto_class_id(v) {
        return proto_hard(cid, op);
    }
    let Some(cid) = class_id_of(v) else {
        return;
    };
    hard_class(cid, op);
}

pub(crate) fn hard_ptr(p: *const crate::object::ObjectHeader, op: &str) {
    let Some(cid) = class_id_at((p as u64 & POINTER_MASK) as usize) else {
        return;
    };
    hard_class(cid, op);
}

fn hard_class(cid: u32, op: &str) {
    let cl = claims();
    CHECKS.fetch_add(1, Ordering::Relaxed);
    if let Some(c) = cl.get(&cid) {
        if c.prefix_stable {
            violate("F3 prefix_stable", c, cid, op, "");
        }
    }
}

/// A HARD operation on the prototype of class `pid`: every claimed class
/// whose chain includes it.
pub(crate) fn proto_hard(pid: u32, op: &str) {
    let cl = claims();
    CHECKS.fetch_add(1, Ordering::Relaxed);
    for (&cid, c) in cl {
        if c.prefix_stable && (cid == pid || c.ancestors.contains(&pid)) {
            violate(
                "F3 prefix_stable",
                c,
                cid,
                op,
                " on a prototype in its chain",
            );
        }
    }
}

/// `Object.create(proto)` (E10).
pub(crate) fn object_create(proto: f64) {
    if let Some(pid) = proto_class_id(proto) {
        let cl = claims();
        CHECKS.fetch_add(1, Ordering::Relaxed);
        if let Some(c) = cl.get(&pid) {
            if c.prefix_stable {
                violate("F3 prefix_stable", c, pid, "Object.create(C.prototype)", "");
            }
        }
    }
}

fn check_store(cid: u32, key: &[u8], value: f64, op: &str, readonly_too: bool) {
    let cl = claims();
    CHECKS.fetch_add(1, Ordering::Relaxed);
    let Some(c) = cl.get(&cid) else {
        return;
    };
    match c.keys.iter().find(|k| k.key == key) {
        Some(k) => {
            if k.kind != 0xFFFF && k.kind & kind_of(value) == 0 {
                violate(
                    "F2 kind",
                    c,
                    cid,
                    op,
                    &format!(
                        " storing a value of kind bits {:#x} into .{} (claimed {:#x})",
                        kind_of(value),
                        String::from_utf8_lossy(key),
                        k.kind
                    ),
                );
            }
            if readonly_too && k.readonly {
                violate(
                    "F1 readonly",
                    c,
                    cid,
                    op,
                    &format!(" writing .{}", String::from_utf8_lossy(key)),
                );
            }
            if c.fixed_layout && !k.birth {
                violate("F3 fixed_layout", c, cid, op, " adding a non-birth key");
            }
        }
        None => {
            if c.fixed_layout {
                violate(
                    "F3 fixed_layout",
                    c,
                    cid,
                    op,
                    &format!(" adding key .{}", String::from_utf8_lossy(key)),
                );
            }
        }
    }
}

/// The generic by-name store slow path.
pub(crate) fn generic_store(
    obj: *const crate::object::ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if key.is_null() {
        return;
    }
    let Some(cid) = class_id_at((obj as u64 & POINTER_MASK) as usize) else {
        return;
    };
    let bytes = unsafe {
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize)
    };
    match RUNTIME_WRITER.with(|w| w.get()) {
        Some(op) => check_store(cid, bytes, value, op, true),
        None => check_store(cid, bytes, value, "a generic store", false),
    }
}

/// A keyed PutValue store (`o[k] = v`, `super[k] = v`, `Object.assign`'s
/// [[Set]]): kind and layout claims always, readonly claims inside a runtime
/// writer. The [[Set]] lookup starts at `target`; a data store lands on
/// `receiver`. A store that meets an accessor or a non-writable property on
/// the way stores nothing on the receiver (a setter's own stores are checked
/// where they happen), and a declared class prototype is not an instance of
/// its class.
pub(crate) fn put_value(target: f64, key: f64, value: f64, receiver: f64) {
    let Some(cid) = class_id_of(receiver) else {
        return;
    };
    if crate::object::class_id_for_decl_prototype_object(
        (receiver.to_bits() & POINTER_MASK) as usize,
    )
    .is_some()
    {
        return;
    }
    let key = if crate::JSValue::from_bits(key.to_bits()).is_short_string() {
        match unsafe { crate::object::store_key::interned_key_for_store(key) } {
            Some(atom) => crate::value::js_nanbox_string(atom as i64),
            None => return,
        }
    } else {
        key
    };
    let Some(k) = key_bytes(key) else {
        return;
    };
    if set_is_intercepted(target, key, &k) {
        return;
    }
    match RUNTIME_WRITER.with(|w| w.get()) {
        Some(op) => check_store(cid, &k, value, op, true),
        None => check_store(cid, &k, value, "a keyed store", false),
    }
}

/// Whether a [[Set]] of the heap-string `key` starting at `target` meets an
/// accessor or non-writable property (on `target` or its prototype chain).
/// A chain the walk cannot classify answers true, so the check stays silent
/// rather than reporting a store it cannot prove happens.
fn set_is_intercepted(target: f64, key: f64, name: &[u8]) -> bool {
    let bits = target.to_bits();
    if bits & TAG_MASK != POINTER_TAG {
        return true;
    }
    let addr = (bits & POINTER_MASK) as usize;
    let Ok(name) = std::str::from_utf8(name) else {
        return true;
    };
    unsafe {
        if crate::value::addr_class::try_read_tracked_gc_header_of_type(
            addr,
            crate::gc::GC_TYPE_OBJECT,
        )
        .is_none()
        {
            return true;
        }
        use crate::object::descriptor_state as ds;
        // The holder `target` itself (a `super` store starts at the home
        // object's parent, which holds the accessor), then its chain.
        let own = if crate::object::key_attrs::attrs_live_in_keys(addr) {
            crate::object::key_attrs::object_key_blocks_plain_store(
                addr as *const crate::object::ObjectHeader,
                name.as_bytes(),
            )
        } else if ds::object_has_descriptors(addr) {
            ds::get_accessor_descriptor(addr, name).is_some()
                || ds::get_property_attrs(addr, name).is_some_and(|a| !a.writable())
        } else {
            false
        };
        own || ds::class_instance_set_may_intercept(addr, 0, key)
    }
}

/// A runtime writer that never runs in construction, writing `key`.
pub(crate) fn runtime_write(target: f64, key: f64, value: f64, op: &'static str) {
    let Some(cid) = class_id_of(target) else {
        return;
    };
    match key_bytes(key) {
        Some(k) => check_store(cid, &k, value, op, true),
        None => {}
    }
}

pub(crate) fn runtime_write_named(target: f64, key: &[u8], value: f64, op: &'static str) {
    let Some(cid) = class_id_of(target) else {
        return;
    };
    check_store(cid, key, value, op, true);
}

/// While alive, generic stores on this thread are runtime-writer stores
/// (they also check readonly claims).
pub(crate) struct RuntimeWriter(Option<&'static str>);

impl RuntimeWriter {
    pub(crate) fn enter(op: &'static str) -> Self {
        RuntimeWriter(RUNTIME_WRITER.with(|w| w.replace(Some(op))))
    }
}

impl Drop for RuntimeWriter {
    fn drop(&mut self) {
        RUNTIME_WRITER.with(|w| w.set(self.0));
    }
}

fn key_bytes(v: f64) -> Option<Vec<u8>> {
    let jsval = crate::value::JSValue::from_bits(v.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(v) as *const crate::StringHeader;
    if !crate::value::addr_class::is_above_handle_band(ptr as usize) {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = crate::string::string_data(ptr);
        Some(std::slice::from_raw_parts(data, len).to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blob in the compiler's `.perry_facts` v2 layout
    /// (`perry-wpfacts` `dump::contract_section`).
    fn blob() -> Vec<u8> {
        blob_with(3, 0b111, K_NUM)
    }

    /// Class 7 with key `x`: class flags, key flags and kind given.
    fn blob_with(flags: u32, key_flags: u8, kind: u16) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(b"PFCT");
        r.extend_from_slice(&2u32.to_le_bytes());
        r.extend_from_slice(&1u32.to_le_bytes());
        r.extend_from_slice(&7u32.to_le_bytes()); // class id
        r.extend_from_slice(&flags.to_le_bytes()); // prefix_stable | fixed_layout
        r.extend_from_slice(&1u16.to_le_bytes()); // one ancestor
        r.extend_from_slice(&5u32.to_le_bytes());
        r.extend_from_slice(&3u16.to_le_bytes());
        r.extend_from_slice(b"a:P");
        r.extend_from_slice(&1u16.to_le_bytes());
        r.extend_from_slice(&1u16.to_le_bytes());
        r.extend_from_slice(b"x");
        r.push(key_flags);
        r.extend_from_slice(&kind.to_le_bytes());
        while r.len() % 8 != 0 {
            r.push(0);
        }
        r
    }

    /// Two records of one class id (two units' copies of a literal class):
    /// the instances are checked against the weakest claim, whichever record
    /// the linker placed last. One copy holds no instance (kind none, every
    /// claim vacuous), the other's are written with strings.
    #[test]
    fn records_of_one_class_meet() {
        let vacuous = blob_with(3, 0b111, 0);
        let written = blob_with(0, 0b010, K_STR);
        for (a, b) in [(&vacuous, &written), (&written, &vacuous)] {
            let mut sec = a.clone();
            sec.extend(b.iter().copied());
            let mut out = HashMap::new();
            parse_into(&sec, &mut out);
            let c = out.get(&7).expect("claim for class 7");
            assert!(!c.prefix_stable && !c.fixed_layout);
            assert_eq!(c.keys.len(), 1);
            assert!(!c.keys[0].readonly && !c.keys[0].birth);
            assert_eq!(c.keys[0].kind, K_STR);
        }
    }

    #[test]
    fn parses_back_to_back_blobs() {
        let mut sec = blob();
        sec.extend(blob());
        let mut out = HashMap::new();
        parse_into(&sec, &mut out);
        let c = out.get(&7).expect("claim for class 7");
        assert!(c.prefix_stable && c.fixed_layout);
        assert_eq!(c.ancestors, vec![5]);
        assert_eq!(c.keys.len(), 1);
        assert!(c.keys[0].readonly && c.keys[0].birth);
        assert_eq!(c.keys[0].kind, K_NUM);
    }

    #[test]
    fn value_kinds_match_the_compiler_mask() {
        assert_eq!(kind_of(1.5), K_NUM);
        assert_eq!(kind_of(f64::from_bits(INT32_TAG | 3)), K_NUM);
        assert_eq!(kind_of(f64::from_bits(TAG_MARKER | 1)), K_UNDEF);
        assert_eq!(kind_of(f64::from_bits(TAG_MARKER | 2)), K_NULL);
        assert_eq!(kind_of(f64::from_bits(TAG_MARKER | 4)), K_BOOL);
        assert_eq!(kind_of(f64::from_bits(STRING_TAG | 0x1000)), K_STR);
        assert_eq!(kind_of(f64::from_bits(BIGINT_TAG | 0x1000)), K_BIGINT);
    }
}
