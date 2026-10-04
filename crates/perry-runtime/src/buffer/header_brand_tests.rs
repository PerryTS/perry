//! #10694: a buffer's flavor is its GC type byte. These pin the properties the
//! old address-keyed registries could not give, and the guards the header read
//! needs that a table lookup did not.

use super::header::*;
use crate::gc::{
    GC_TYPE_BUFFER, GC_TYPE_BUFFER_ARRAY_BUFFER, GC_TYPE_BUFFER_CRYPTO_KEY,
    GC_TYPE_BUFFER_SECRET_KEY, GC_TYPE_BUFFER_UINT8ARRAY,
};

fn brand(addr: usize) -> Option<u8> {
    buffer_family_type(addr)
}

/// A brand written on one thread is read on another with no publication step:
/// it is in the cell. The old design needed process-global `EXTERNAL_*` sets
/// (and a latch that every inserter had to arm, #9176) for exactly this case,
/// because perry-stdlib marks buffers off the main thread.
#[test]
fn a_brand_written_on_one_thread_is_seen_on_another() {
    let u8a = buffer_alloc(8) as usize;
    let key = buffer_alloc(32) as usize;
    std::thread::spawn(move || {
        js_buffer_mark_as_uint8array_external(u8a);
        js_buffer_mark_as_crypto_key_external(key, 1, 2, 1, 1, 0, 256);
    })
    .join()
    .expect("marking thread");
    assert!(is_uint8array_buffer(u8a));
    assert_eq!(brand(u8a), Some(GC_TYPE_BUFFER_UINT8ARRAY));
    assert_eq!(brand(key), Some(GC_TYPE_BUFFER_CRYPTO_KEY));
    assert!(
        is_uint8array_buffer(key),
        "a CryptoKey's storage is Uint8Array-backed"
    );
    assert_eq!(
        crypto_key_meta(key).map(|m| (m.0, m.1, m.5)),
        Some((1, 2, 256))
    );
    let seen_elsewhere = std::thread::spawn(move || (is_uint8array_buffer(u8a), brand(key)))
        .join()
        .expect("probe thread");
    assert_eq!(seen_elsewhere, (true, Some(GC_TYPE_BUFFER_CRYPTO_KEY)));
}

/// Branding is a statement about a cell that is already a buffer. A "mark" on
/// anything else is refused, so no address can be made to answer as a buffer
/// by a stray registration (the forged-registry tests this replaces needed a
/// separate header validation for that).
#[test]
fn a_brand_cannot_be_written_onto_a_non_buffer_cell() {
    let obj = crate::object::js_object_alloc(0, 2) as usize;
    mark_as_uint8array(obj);
    mark_as_array_buffer(obj);
    mark_as_secret_key(obj);
    mark_as_crypto_key(obj, 1, 2, 1);
    assert_eq!(brand(obj), None);
    assert!(!is_registered_buffer(obj));
    assert_eq!(crypto_key_meta(obj), None);
    let header =
        unsafe { crate::value::addr_class::try_read_gc_header(obj) }.expect("object header");
    assert_eq!(
        header.obj_type,
        crate::gc::GC_TYPE_OBJECT,
        "the object kept its own type"
    );
}

/// A key cell is Uint8Array storage under a more specific brand; a later
/// Uint8Array mark (every key producer issues one first, some after) must not
/// demote it, and a CryptoKey stays a CryptoKey.
#[test]
fn a_uint8array_mark_does_not_demote_a_key_brand() {
    let secret = buffer_alloc(16) as usize;
    mark_as_uint8array(secret);
    mark_as_secret_key(secret);
    mark_as_uint8array(secret);
    assert_eq!(brand(secret), Some(GC_TYPE_BUFFER_SECRET_KEY));
    assert!(is_secret_key(secret));

    let crypto = buffer_alloc(16) as usize;
    mark_as_crypto_key(crypto, 1, 2, 1);
    mark_as_secret_key(crypto);
    mark_as_uint8array(crypto);
    assert_eq!(brand(crypto), Some(GC_TYPE_BUFFER_CRYPTO_KEY));
    assert!(crypto_key_meta(crypto).is_some());

    let plain = buffer_alloc(16) as usize;
    assert_eq!(
        brand(plain),
        Some(GC_TYPE_BUFFER),
        "buffer_alloc births a Node Buffer"
    );
    mark_as_array_buffer(plain);
    assert_eq!(brand(plain), Some(GC_TYPE_BUFFER_ARRAY_BUFFER));
    assert!(!is_uint8array_buffer(plain));
}

/// The one POINTER-tagged value with no `GcHeader` is a `Box`-leaked symbol,
/// whose `addr - 8` can hold any byte. Forge exactly that: a non-GC block whose
/// "header" says Uint8Array and whose first word is `SYMBOL_MAGIC`. The header
/// read alone would call it a buffer; the symbol screen plus the ownership
/// check must not. Delete either and this fails.
#[test]
fn a_headerless_symbol_whose_preceding_byte_looks_like_a_brand_is_not_a_buffer() {
    let block: Box<[u64; 4]> = Box::new([
        GC_TYPE_BUFFER_UINT8ARRAY as u64,
        crate::symbol::SYMBOL_MAGIC as u64,
        0,
        0,
    ]);
    let base = Box::into_raw(block) as usize;
    let addr = base + crate::gc::GC_HEADER_SIZE;
    assert_eq!(
        unsafe { crate::value::addr_class::try_read_gc_header(addr) }.map(|h| h.obj_type),
        Some(GC_TYPE_BUFFER_UINT8ARRAY),
        "fixture premise: the bare header read sees a buffer brand"
    );
    assert!(!is_registered_buffer(addr));
    assert!(!is_uint8array_buffer(addr));
    assert_eq!(crate::typedarray::lookup_typed_array_kind(addr), None);
    drop(unsafe { Box::from_raw(base as *mut [u64; 4]) });
}

/// A real buffer whose `length` equals `SYMBOL_MAGIC` passes the screen's
/// "maybe a symbol" arm and must still be recognised through the ownership
/// check (only the length word is set; no bytes are touched).
#[test]
fn a_buffer_whose_length_word_equals_the_symbol_magic_is_still_a_buffer() {
    let buf = buffer_alloc(8);
    let saved = unsafe { (*buf).length };
    unsafe { (*buf).length = crate::symbol::SYMBOL_MAGIC };
    let seen = is_registered_buffer(buf as usize);
    unsafe { (*buf).length = saved };
    assert!(seen);
}
