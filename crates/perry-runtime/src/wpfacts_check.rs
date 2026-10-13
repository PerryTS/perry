//! wpfacts slice 2 contract witness: the run-time check of one elided
//! field access (design 6.1).
//!
//! The zero-guard tier (`perry-codegen` `expr/wpfacts_access.rs`) lowers a
//! field read or write whose receiver the whole-program facts prove to a
//! bare slot load/store. Compiled with `PERRY_CHECK_CONTRACTS=1`, every such
//! site first calls this function with what it relied on: the receiver's
//! class set (F4), the key's birth slot (F3), the value kind (F2). The
//! function re-derives each from the runtime's own view of the object and
//! aborts, naming the site, when one does not hold. It reads only and runs
//! no user code (its key-list read goes through the runtime's array-length
//! helper, so the GC effects table classes it `Reenters`).
//!
//! Always compiled (not behind `contract-checks`): a program built with the
//! prebuilt runtime (`PERRY_NO_AUTO_OPTIMIZE`) must link its witnesses too.
//! Release codegen never calls it.

use crate::object::ObjectHeader;
use crate::StringHeader;

const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;
const TAG_MARKER: u64 = 0x7FFC_0000_0000_0000;

const K_NUM: u32 = 1;
const K_STR: u32 = 2;
const K_BOOL: u32 = 4;
const K_NULL: u32 = 8;
const K_UNDEF: u32 = 16;
const K_BIGINT: u32 = 32;
const K_SYMBOL: u32 = 64;
const K_OBJ: u32 = 128;

/// `flags` bits (must match `perry-codegen` `expr/wpfacts_access.rs`).
pub const WPF_STORE: u32 = 1;
pub const WPF_NULLABLE: u32 = 2;
/// The site reads the slot as a raw double (F64 lane, number-only kind).
pub const WPF_RAW_F64: u32 = 4;

/// The kind bit of a NaN-boxed value (compiler `KindFact::mask` layout).
fn kind_of(bits: u64) -> u32 {
    match bits & TAG_MASK {
        POINTER_TAG => {
            if unsafe { crate::symbol::js_is_symbol(f64::from_bits(bits)) } != 0 {
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

fn string_bytes(ptr: *const StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = crate::string::string_data(ptr);
        Some(std::slice::from_raw_parts(data, len).to_vec())
    }
}

/// The bytes of a key in an object's key list, without materializing or
/// interning anything (heap string or SSO immediate).
fn key_value_bytes(bits: u64) -> Option<Vec<u8>> {
    match bits & TAG_MASK {
        STRING_TAG => string_bytes((bits & POINTER_MASK) as *const StringHeader),
        SHORT_STRING_TAG => {
            let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = crate::JSValue::from_bits(bits).short_string_to_buf(&mut buf);
            Some(buf[..n].to_vec())
        }
        _ => None,
    }
}

static CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static REPORT: std::sync::Once = std::sync::Once::new();

extern "C" fn report_at_exit() {
    eprintln!(
        "perry: wpfacts elided-site witnesses: {} checks, 0 violations",
        CHECKS.load(std::sync::atomic::Ordering::Relaxed)
    );
}

#[cold]
#[inline(never)]
fn violate(site: u64, what: &str, detail: String) -> ! {
    eprintln!("perry: wpfacts contract violated at elided site {site:#018x}: {what} ({detail})");
    std::process::abort();
}

/// One elided field access. `cids01`/`cids23` carry up to four class ids
/// (low half first; `u32::MAX` = none). Returns 0.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn perry_wpfacts_check_field(
    obj: f64,
    value: f64,
    key: i64,
    cids01: i64,
    cids23: i64,
    slot: i32,
    kind_mask: i32,
    flags: i32,
    site: i64,
) -> i32 {
    let (site, flags, slot, kind_mask) = (site as u64, flags as u32, slot as u32, kind_mask as u32);
    if CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
        REPORT.call_once(|| {
            if std::env::var_os("PERRY_CONTRACTS_REPORT").is_some() {
                unsafe {
                    libc::atexit(report_at_exit);
                }
            }
        });
    }
    let bits = obj.to_bits();
    let key_ptr = key as usize as *const StringHeader;
    let key_text = string_bytes(key_ptr).unwrap_or_default();
    let key_name = String::from_utf8_lossy(&key_text).into_owned();
    if bits & TAG_MASK != POINTER_TAG {
        let k = kind_of(bits);
        if flags & WPF_NULLABLE != 0 && (k == K_NULL || k == K_UNDEF) {
            // The site kept its receiver test; nothing was elided here.
            return 0;
        }
        violate(
            site,
            "F4 receiver set",
            format!(".{key_name}: receiver bits {bits:#018x} are not an object"),
        );
    }
    let addr = (bits & POINTER_MASK) as usize;
    let cid = unsafe {
        if addr < 0x1000
            || crate::value::addr_class::try_read_tracked_gc_header_of_type(
                addr,
                crate::gc::GC_TYPE_OBJECT,
            )
            .is_none()
        {
            violate(
                site,
                "F4 receiver set",
                format!(".{key_name}: receiver {addr:#x} is not an ordinary object"),
            );
        }
        (*(addr as *const ObjectHeader)).class_id
    };
    let ids = [
        cids01 as u64 as u32,
        (cids01 as u64 >> 32) as u32,
        cids23 as u64 as u32,
        (cids23 as u64 >> 32) as u32,
    ];
    if !ids.iter().any(|&c| c != u32::MAX && c == cid) {
        violate(
            site,
            "F4 receiver set",
            format!(".{key_name}: receiver class_id {cid} not in {ids:?}"),
        );
    }
    // F3: the key lives at the claimed slot of the object's real layout.
    let (keys, live) = unsafe { crate::object::object_keys_and_live_slot_count(addr as *const _) };
    if slot >= live || slot >= keys.count() {
        violate(
            site,
            "F3 slot",
            format!(
                ".{key_name}: slot {slot} beyond the layout (live {live}, keys {}) of class_id {cid}",
                keys.count()
            ),
        );
    }
    let at = unsafe { keys.get(slot) }.bits();
    if key_value_bytes(at).as_deref() != Some(key_text.as_slice()) {
        violate(
            site,
            "F3 slot",
            format!(
                ".{key_name}: slot {slot} of class_id {cid} holds key {:?}",
                key_value_bytes(at).map(|b| String::from_utf8_lossy(&b).into_owned())
            ),
        );
    }
    // F2: the value read or written is inside the claimed kind; the value in
    // the slot is the one the site saw.
    let header = std::mem::size_of::<ObjectHeader>();
    let in_slot = unsafe { *((addr + header + slot as usize * 8) as *const u64) };
    let vbits = value.to_bits();
    if flags & WPF_RAW_F64 != 0 {
        // A raw-f64 read: the slot must hold a plain double (any NaN is the
        // canonical one).
        if vbits & TAG_MASK >= 0x7FF9_0000_0000_0000 && vbits != 0x7FF8_0000_0000_0000 {
            violate(
                site,
                "F2 kind (raw f64)",
                format!(".{key_name}: slot holds tagged bits {vbits:#018x}"),
            );
        }
    }
    if in_slot != vbits && flags & WPF_STORE == 0 {
        violate(
            site,
            "F3 slot",
            format!(".{key_name}: loaded {vbits:#018x}, slot now {in_slot:#018x}"),
        );
    }
    let k = kind_of(in_slot);
    if k & kind_mask == 0 {
        violate(
            site,
            "F2 kind",
            format!(".{key_name}: value {in_slot:#018x} (kind {k:#x}) outside mask {kind_mask:#x}"),
        );
    }
    0
}
