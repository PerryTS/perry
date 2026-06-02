//! `Stats` / bigint-`Stats` object + `statSync` / `lstatSync`.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt};

use super::*;

// ---------- Stats object ----------
//
// `fs.statSync(path)` returns a Node-style Stats object supporting
// Node's predicate methods and scalar/timestamp fields. We implement it as a
// plain ObjectHeader populated with closure fields for the predicates. The
// closures capture a pre-computed boolean result so calling them just returns
// the stored value via `js_closure_get_capture_f64`.

pub(crate) extern "C" fn stats_closure_return_captured(
    closure: *const crate::closure::ClosureHeader,
) -> f64 {
    // Slot 0 holds the pre-computed NaN-boxed boolean.
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

pub(crate) unsafe fn make_stats_predicate(value: bool) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let tag = if value { TAG_TRUE } else { TAG_FALSE };
    let closure = crate::closure::js_closure_alloc(stats_closure_return_captured as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(tag));
    // NaN-box the closure pointer with POINTER_TAG so the dynamic
    // dispatch path in `js_native_call_method` can unwrap it.
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    f64::from_bits(POINTER_TAG | (closure as u64 & 0x0000_FFFF_FFFF_FFFF))
}

pub(crate) fn bigint_u64_value(value: u64) -> f64 {
    let ptr = crate::bigint::js_bigint_from_u64(value);
    crate::value::js_nanbox_bigint(ptr as i64)
}

pub(crate) fn bigint_i64_value(value: i64) -> f64 {
    let ptr = crate::bigint::js_bigint_from_i64(value);
    crate::value::js_nanbox_bigint(ptr as i64)
}

// Pre-packed Stats key lists. Null-separated bytes are the format
// `js_object_alloc_class_with_keys` expects; the shape cache builds the
// JS keys array once and reuses it across every `statSync` invocation.
//
// Class IDs are reserved for Perry's runtime-internal Stats shapes:
//   - 0xFE5C: regular Stats (numeric fields)
//   - 0xFE5D: bigint Stats (adds *Ns fields)
//
// Field order MUST match the order writes are emitted below. The Date aliases
// live in hidden slots after the enumerable fields and are exposed through
// class-level getters/setters below.
pub(crate) const STATS_KEYS_REGULAR: &[u8] = b"isFile\0isDirectory\0isSymbolicLink\0isBlockDevice\0isCharacterDevice\0isFIFO\0isSocket\0size\0atimeMs\0mtimeMs\0ctimeMs\0birthtimeMs\0mode\0uid\0gid\0nlink\0dev\0rdev\0blksize\0ino\0blocks\0";
pub(crate) const STATS_REGULAR_COUNT: u32 = 25;
pub(crate) const STATS_REGULAR_CLASS_ID: u32 = 0xFFFF_0070;

pub(crate) const STATS_KEYS_BIGINT: &[u8] = b"isFile\0isDirectory\0isSymbolicLink\0isBlockDevice\0isCharacterDevice\0isFIFO\0isSocket\0size\0atimeMs\0mtimeMs\0ctimeMs\0birthtimeMs\0atimeNs\0mtimeNs\0ctimeNs\0birthtimeNs\0mode\0uid\0gid\0nlink\0dev\0rdev\0blksize\0ino\0blocks\0";
pub(crate) const STATS_BIGINT_COUNT: u32 = 29;
pub(crate) const STATS_BIGINT_CLASS_ID: u32 = 0xFFFF_0071;

const REGULAR_DATE_SLOT_BASE: u32 = 21;
const BIGINT_DATE_SLOT_BASE: u32 = 25;

fn stats_date_slot(
    this_value: f64,
    offset: u32,
) -> Option<(*mut crate::object::ObjectHeader, u32)> {
    let value = crate::value::JSValue::from_bits(this_value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let obj = value.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    if obj.is_null() {
        return None;
    }
    unsafe {
        match (*obj).class_id {
            STATS_REGULAR_CLASS_ID => Some((obj, REGULAR_DATE_SLOT_BASE + offset)),
            STATS_BIGINT_CLASS_ID => Some((obj, BIGINT_DATE_SLOT_BASE + offset)),
            _ => None,
        }
    }
}

fn stats_date_get(this_value: f64, offset: u32) -> f64 {
    if let Some((obj, slot)) = stats_date_slot(this_value, offset) {
        return f64::from_bits(crate::object::js_object_get_field(obj, slot).bits());
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn stats_date_set(this_value: f64, offset: u32, value: f64) -> f64 {
    if let Some((obj, slot)) = stats_date_slot(this_value, offset) {
        crate::object::js_object_set_field_f64(obj, slot, value);
    }
    value
}

extern "C" fn stats_atime_getter(this_value: f64) -> f64 {
    stats_date_get(this_value, 0)
}

extern "C" fn stats_mtime_getter(this_value: f64) -> f64 {
    stats_date_get(this_value, 1)
}

extern "C" fn stats_ctime_getter(this_value: f64) -> f64 {
    stats_date_get(this_value, 2)
}

extern "C" fn stats_birthtime_getter(this_value: f64) -> f64 {
    stats_date_get(this_value, 3)
}

extern "C" fn stats_atime_setter(this_value: f64, value: f64) -> f64 {
    stats_date_set(this_value, 0, value)
}

extern "C" fn stats_mtime_setter(this_value: f64, value: f64) -> f64 {
    stats_date_set(this_value, 1, value)
}

extern "C" fn stats_ctime_setter(this_value: f64, value: f64) -> f64 {
    stats_date_set(this_value, 2, value)
}

extern "C" fn stats_birthtime_setter(this_value: f64, value: f64) -> f64 {
    stats_date_set(this_value, 3, value)
}

fn ensure_stats_date_accessors_registered() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| unsafe {
        for class_id in [STATS_REGULAR_CLASS_ID, STATS_BIGINT_CLASS_ID] {
            for (name, getter, setter) in [
                (
                    "atime",
                    stats_atime_getter as *const u8,
                    stats_atime_setter as *const u8,
                ),
                (
                    "mtime",
                    stats_mtime_getter as *const u8,
                    stats_mtime_setter as *const u8,
                ),
                (
                    "ctime",
                    stats_ctime_getter as *const u8,
                    stats_ctime_setter as *const u8,
                ),
                (
                    "birthtime",
                    stats_birthtime_getter as *const u8,
                    stats_birthtime_setter as *const u8,
                ),
            ] {
                crate::object::js_register_class_getter(
                    class_id as i64,
                    name.as_ptr(),
                    name.len() as i64,
                    getter as i64,
                );
                crate::object::js_register_class_setter(
                    class_id as i64,
                    name.as_ptr(),
                    name.len() as i64,
                    setter as i64,
                );
            }
        }
    });
}

pub(crate) unsafe fn build_stats_object(
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
    size: u64,
    mode: u32,
    uid: f64,
    gid: f64,
    nlink: f64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
    bigint: bool,
    meta_extra: Option<&fs::Metadata>,
) -> f64 {
    let (dev, rdev, blksize, ino, blocks) = metadata_node_extra_fields(meta_extra);
    let (is_block_device, is_character_device, is_fifo, is_socket) =
        metadata_special_file_predicates(meta_extra);
    // Real nanosecond timestamps when we have a Metadata in hand; otherwise
    // fall back to the millisecond × 1e6 approximation below.
    let times_ns = meta_extra.map(metadata_times_ns);
    ensure_stats_date_accessors_registered();

    let (obj, count) = if bigint {
        let o = crate::object::js_object_alloc_class_with_keys(
            STATS_BIGINT_CLASS_ID,
            0,
            STATS_BIGINT_COUNT,
            STATS_KEYS_BIGINT.as_ptr(),
            (STATS_KEYS_BIGINT.len() - 1) as u32,
        );
        (o, STATS_BIGINT_COUNT)
    } else {
        let o = crate::object::js_object_alloc_class_with_keys(
            STATS_REGULAR_CLASS_ID,
            0,
            STATS_REGULAR_COUNT,
            STATS_KEYS_REGULAR.as_ptr(),
            (STATS_KEYS_REGULAR.len() - 1) as u32,
        );
        (o, STATS_REGULAR_COUNT)
    };
    let _ = count;
    let set = |idx: u32, v: f64| {
        crate::object::js_object_set_field_f64(obj, idx, v);
    };
    let set_date_aliases = |base: u32| {
        set(base, crate::date::alloc_date_cell(atime_ms));
        set(base + 1, crate::date::alloc_date_cell(mtime_ms));
        set(base + 2, crate::date::alloc_date_cell(ctime_ms));
        set(base + 3, crate::date::alloc_date_cell(birthtime_ms));
    };
    set(0, make_stats_predicate(is_file));
    set(1, make_stats_predicate(is_dir));
    set(2, make_stats_predicate(is_symlink));
    set(3, make_stats_predicate(is_block_device));
    set(4, make_stats_predicate(is_character_device));
    set(5, make_stats_predicate(is_fifo));
    set(6, make_stats_predicate(is_socket));
    if bigint {
        let (a_ns, m_ns, c_ns, b_ns) = times_ns.unwrap_or((
            (atime_ms as i64).saturating_mul(1_000_000),
            (mtime_ms as i64).saturating_mul(1_000_000),
            (ctime_ms as i64).saturating_mul(1_000_000),
            (birthtime_ms as i64).saturating_mul(1_000_000),
        ));
        let ns_to_ms = |ns: i64| ns / 1_000_000;
        set(7, bigint_u64_value(size));
        set(8, bigint_i64_value(ns_to_ms(a_ns)));
        set(9, bigint_i64_value(ns_to_ms(m_ns)));
        set(10, bigint_i64_value(ns_to_ms(c_ns)));
        set(11, bigint_i64_value(ns_to_ms(b_ns)));
        set(12, bigint_i64_value(a_ns));
        set(13, bigint_i64_value(m_ns));
        set(14, bigint_i64_value(c_ns));
        set(15, bigint_i64_value(b_ns));
        set(16, bigint_u64_value(mode as u64));
        set(17, bigint_i64_value(uid as i64));
        set(18, bigint_i64_value(gid as i64));
        set(19, bigint_i64_value(nlink as i64));
        set(20, bigint_u64_value(dev));
        set(21, bigint_u64_value(rdev));
        set(22, bigint_u64_value(blksize));
        set(23, bigint_u64_value(ino));
        set(24, bigint_u64_value(blocks));
        set_date_aliases(BIGINT_DATE_SLOT_BASE);
    } else {
        set(7, size as f64);
        set(8, atime_ms);
        set(9, mtime_ms);
        set(10, ctime_ms);
        set(11, birthtime_ms);
        set(12, mode as f64);
        set(13, uid);
        set(14, gid);
        set(15, nlink);
        set(16, dev as f64);
        set(17, rdev as f64);
        set(18, blksize as f64);
        set(19, ino as f64);
        set(20, blocks as f64);
        set_date_aliases(REGULAR_DATE_SLOT_BASE);
    }
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    f64::from_bits(POINTER_TAG | (obj as u64 & 0x0000_FFFF_FFFF_FFFF))
}

pub(crate) fn system_time_ms(time: std::io::Result<std::time::SystemTime>) -> f64 {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

pub(crate) fn metadata_times_ms(meta: &fs::Metadata) -> (f64, f64, f64, f64) {
    let atime = system_time_ms(meta.accessed());
    let mtime = system_time_ms(meta.modified());
    let birth = system_time_ms(meta.created());
    let ctime = unix_ctime_ms(meta).unwrap_or(mtime);
    (atime, mtime, ctime, birth)
}

#[cfg(unix)]
pub(crate) fn unix_ctime_ms(meta: &fs::Metadata) -> Option<f64> {
    // `MetadataExt::ctime` is seconds since epoch; combine with the
    // nanosecond fraction so we don't drop sub-second precision in the
    // ms conversion. Matches Node's stat.ctimeMs on POSIX.
    let secs = meta.ctime();
    let nsecs = meta.ctime_nsec().max(0) as f64;
    Some(secs as f64 * 1000.0 + nsecs / 1_000_000.0)
}

#[cfg(not(unix))]
pub(crate) fn unix_ctime_ms(_meta: &fs::Metadata) -> Option<f64> {
    None
}

/// Nanosecond timestamps for `bigint: true` Stats. On Unix we read the
/// real `*time_nsec` fields directly; elsewhere we fall back to the
/// millisecond × 1_000_000 approximation.
#[cfg(unix)]
pub(crate) fn metadata_times_ns(meta: &fs::Metadata) -> (i64, i64, i64, i64) {
    let to_ns = |secs: i64, nsecs: i64| -> i64 {
        secs.saturating_mul(1_000_000_000)
            .saturating_add(nsecs.max(0))
    };
    let a = to_ns(meta.atime(), meta.atime_nsec());
    let m = to_ns(meta.mtime(), meta.mtime_nsec());
    let c = to_ns(meta.ctime(), meta.ctime_nsec());
    // birthtime is not always available via MetadataExt across Unixen;
    // when unset fall back to a derived value from `created()`.
    let birth = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(m);
    (a, m, c, birth)
}

#[cfg(not(unix))]
pub(crate) fn metadata_times_ns(meta: &fs::Metadata) -> (i64, i64, i64, i64) {
    let to_ns = |ms: f64| -> i64 {
        if ms.is_finite() {
            (ms as i64).saturating_mul(1_000_000)
        } else {
            0
        }
    };
    let (atime_ms, mtime_ms, ctime_ms, birthtime_ms) = metadata_times_ms(meta);
    (
        to_ns(atime_ms),
        to_ns(mtime_ms),
        to_ns(ctime_ms),
        to_ns(birthtime_ms),
    )
}

pub(crate) fn metadata_owner_ids(meta: &fs::Metadata) -> (f64, f64) {
    #[cfg(unix)]
    {
        (meta.uid() as f64, meta.gid() as f64)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (-1.0, -1.0)
    }
}

pub(crate) fn metadata_nlink(meta: &fs::Metadata) -> f64 {
    #[cfg(unix)]
    {
        meta.nlink() as f64
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        1.0
    }
}

pub(crate) fn metadata_node_extra_fields(meta: Option<&fs::Metadata>) -> (u64, u64, u64, u64, u64) {
    #[cfg(unix)]
    {
        if let Some(meta) = meta {
            return (
                meta.dev(),
                meta.rdev(),
                meta.blksize(),
                meta.ino(),
                meta.blocks(),
            );
        }
    }
    let _ = meta;
    (0, 0, 0, 0, 0)
}

fn metadata_special_file_predicates(meta: Option<&fs::Metadata>) -> (bool, bool, bool, bool) {
    #[cfg(unix)]
    {
        if let Some(meta) = meta {
            let ft = meta.file_type();
            return (
                ft.is_block_device(),
                ft.is_char_device(),
                ft.is_fifo(),
                ft.is_socket(),
            );
        }
    }
    let _ = meta;
    (false, false, false, false)
}

/// `fs.statSync(path)` — returns a Stats-like object with Node-compatible
/// predicate methods and scalar fields, or throws a Node-shaped fs Error when
/// metadata lookup fails.
#[no_mangle]
pub extern "C" fn js_fs_stat_sync(path_value: f64) -> f64 {
    js_fs_stat_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_stat_sync_options(path_value: f64, options_value: f64) -> f64 {
    crate::fs::validate::validate_path("path", path_value);
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => {
                return build_stats_object(
                    false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
                )
            }
        };
        match fs::metadata(&path_str) {
            Ok(meta) => {
                let is_file = meta.is_file();
                let is_dir = meta.is_dir();
                let is_symlink = meta.file_type().is_symlink();
                let size = meta.len();
                #[cfg(unix)]
                let mode = meta.permissions().mode();
                #[cfg(not(unix))]
                let mode = if meta.permissions().readonly() {
                    0o444
                } else {
                    0o666
                };
                let (uid, gid) = metadata_owner_ids(&meta);
                let nlink = metadata_nlink(&meta);
                let (atime, mtime, ctime, birth) = metadata_times_ms(&meta);
                build_stats_object(
                    is_file,
                    is_dir,
                    is_symlink,
                    size,
                    mode,
                    uid,
                    gid,
                    nlink,
                    atime,
                    mtime,
                    ctime,
                    birth,
                    bigint,
                    Some(&meta),
                )
            }
            Err(err) => {
                let err_val = build_fs_error_value(&err, "stat", &path_str);
                crate::exception::js_throw(err_val)
            }
        }
    }
}

/// `fs.lstatSync(path)` — same Stats shape as `statSync`, but uses
/// symlink metadata so `isSymbolicLink()` works for links.
#[no_mangle]
pub extern "C" fn js_fs_lstat_sync(path_value: f64) -> f64 {
    js_fs_lstat_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_lstat_sync_options(path_value: f64, options_value: f64) -> f64 {
    crate::fs::validate::validate_path("path", path_value);
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => {
                return build_stats_object(
                    false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
                )
            }
        };
        match fs::symlink_metadata(&path_str) {
            Ok(meta) => {
                let ft = meta.file_type();
                let size = meta.len();
                #[cfg(unix)]
                let mode = meta.permissions().mode();
                #[cfg(not(unix))]
                let mode = if meta.permissions().readonly() {
                    0o444
                } else {
                    0o666
                };
                let (uid, gid) = metadata_owner_ids(&meta);
                let nlink = metadata_nlink(&meta);
                let (atime, mtime, ctime, birth) = metadata_times_ms(&meta);
                build_stats_object(
                    ft.is_file(),
                    ft.is_dir(),
                    ft.is_symlink(),
                    size,
                    mode,
                    uid,
                    gid,
                    nlink,
                    atime,
                    mtime,
                    ctime,
                    birth,
                    bigint,
                    Some(&meta),
                )
            }
            Err(err) => {
                let err_val = build_fs_error_value(&err, "lstat", &path_str);
                crate::exception::js_throw(err_val)
            }
        }
    }
}
