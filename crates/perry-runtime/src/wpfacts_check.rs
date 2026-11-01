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

/// A plain double, not a NaN-boxed tagged value: the tags are exactly the
/// positive quiet-NaN prefixes 0x7FF9..=0x7FFF. Negative doubles (sign bit set,
/// -0.0 = 0x8000_0000_0000_0000 included) and the canonical NaN are plain.
fn is_plain_f64(bits: u64) -> bool {
    !(SHORT_STRING_TAG..=STRING_TAG | !TAG_MASK).contains(&bits)
}

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
    if !crate::value::addr_class::is_above_handle_band(ptr as usize) {
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
/// (low half first; `u32::MAX` = none); `slot` names the key, `off` is the
/// byte offset from the object the access used. Returns 0.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn perry_wpfacts_check_field(
    obj: f64,
    value: f64,
    key: i64,
    cids01: i64,
    cids23: i64,
    slot: i32,
    off: i32,
    kind_mask: i32,
    flags: i32,
    site: i64,
) -> i32 {
    let (site, flags, slot, off, kind_mask) = (
        site as u64,
        flags as u32,
        slot as u32,
        off as u32,
        kind_mask as u32,
    );
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
        if crate::value::addr_class::is_handle_band(addr)
            || crate::value::addr_class::try_read_tracked_gc_header_of_type(
                addr,
                crate::gc_object_type_pat!(),
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
    let word = unsafe { (*(addr as *const ObjectHeader)).parent_class_id };
    let ids = [
        cids01 as u64 as u32,
        (cids01 as u64 >> 32) as u32,
        cids23 as u64 as u32,
        (cids23 as u64 >> 32) as u32,
    ];
    // The receiver is a proven class by its class id, or by its layout word:
    // a literal family keeps its late keys and names class 0, so its objects
    // carry the family descriptor word of the proven class instead.
    let admitted = ids.iter().any(|&c| {
        c != u32::MAX
            && (c == cid || crate::object::layout_desc::word_is_class_family_layout(word, c))
    });
    if !admitted {
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
    let in_slot = unsafe { *crate::object::header::object_slot_at(addr as *mut ObjectHeader, off) };
    let vbits = value.to_bits();
    if flags & WPF_RAW_F64 != 0 {
        // A raw-f64 read: the slot must hold a plain double.
        if !is_plain_f64(vbits) {
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

/// One elided call (FVA): a call through a value or a field whose callee
/// set the facts prove, lowered to a direct call of a body or to one
/// `info.code` load and an indirect call. The callee must be a live function
/// object whose body is plain (no rest, no `arguments`) and declares at most
/// `argc` parameters (the arguments the site passes, padded). Returns the
/// body's info: the site compares it with the proven bodies' infos and calls
/// [`perry_wpfacts_check_call_miss`] when it is none of them.
#[no_mangle]
pub extern "C" fn perry_wpfacts_check_call(
    callee: f64,
    argc: i64,
    site: i64,
) -> *const crate::closure::JsFunctionInfo {
    let site = site as u64;
    CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let bits = callee.to_bits();
    if bits & TAG_MASK != POINTER_TAG {
        violate(
            site,
            "callee set",
            format!("callee bits {bits:#018x} are not a function object"),
        );
    }
    let closure = (bits & POINTER_MASK) as usize as *const crate::closure::ClosureHeader;
    let Some(info) = crate::closure::closure_info(closure) else {
        violate(
            site,
            "callee set",
            format!("callee {closure:p} is not a function object"),
        );
    };
    let params = info.plain_params();
    if params == crate::codegen_abi::NOT_PLAIN || i64::from(params) > argc {
        violate(
            site,
            "call form",
            format!("the body declares {params} plain parameters, the site passes {argc}"),
        );
    }
    info
}

/// The callee's body is none of the proven set (`perry_wpfacts_check_call`).
#[no_mangle]
pub extern "C" fn perry_wpfacts_check_call_miss(
    info: *const crate::closure::JsFunctionInfo,
    site: i64,
) -> i32 {
    violate(
        site as u64,
        "callee set",
        format!("body info {info:p} is not a proven body"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_f64_plain_bits() {
        for v in [
            0.0,
            -0.0,
            1.5,
            -1.5,
            -1e300,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NAN,
        ] {
            assert!(is_plain_f64(v.to_bits()), "{v} ({:#018x})", v.to_bits());
        }
        assert!(is_plain_f64(f64::MIN.to_bits()));
        for tag in [
            SHORT_STRING_TAG,
            BIGINT_TAG,
            JS_HANDLE_TAG,
            TAG_MARKER,
            POINTER_TAG,
            INT32_TAG,
            STRING_TAG,
        ] {
            assert!(!is_plain_f64(tag | 0x1234), "{tag:#018x}");
            assert!(!is_plain_f64(tag | POINTER_MASK), "{tag:#018x}");
        }
    }
}
